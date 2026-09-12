//! The session-local ownership state a paired connection's pipeline and
//! the switch-key runner both need: this daemon's own [`InputRole`] and
//! the monotonic generation guarding `ChannelMessage::SwitchOwnership`
//! against stale/duplicate delivery ("Complete Flow V1" task §5's
//! idempotent ownership update: "incoming generation <= current
//! generation -> ignore").
//!
//! Deliberately *not* derived from `DaemonService`'s `devices` list: a
//! local device's own `Active` flag can't tell "no peer pipeline has
//! ever run" apart from "the peer is Primary and this side is
//! Secondary" — both leave this side's own device `Active` in its own
//! view (an `apply_peer_ownership(peer, Secondary)` call sets `Active =
//! LOCAL_DEVICE_ID`, the same value a never-switched daemon starts
//! with). `OwnershipHandle` is the single explicit source of truth for
//! that distinction — cheap to clone and share between
//! `DaemonService`, `pipeline::run_paired_connection`, and
//! `hotkey::runner`, all reading/writing the same underlying atomics.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use flow_core::protocol::InputRole;

/// Cheap, `Send + Sync`; every clone shares the same underlying atomics.
#[derive(Clone)]
pub struct OwnershipHandle {
    is_primary: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
}

impl Default for OwnershipHandle {
    /// A daemon with no live peer pipeline is `Primary` of its own input
    /// by definition — nobody else is controlling it, so nothing stops
    /// it from being the one to initiate the first switch. Task §4's
    /// "deterministic restart behavior": always `Primary` on a cold
    /// start, in-memory only, never persisted.
    fn default() -> Self {
        Self {
            is_primary: Arc::new(AtomicBool::new(true)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl OwnershipHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// This daemon's own role right now.
    pub fn role(&self) -> InputRole {
        if self.is_primary() {
            InputRole::Primary
        } else {
            InputRole::Secondary
        }
    }

    pub fn is_primary(&self) -> bool {
        self.is_primary.load(Ordering::SeqCst)
    }

    pub fn is_secondary(&self) -> bool {
        !self.is_primary()
    }

    /// In the V1 exactly-two-device model the peer's role is always this
    /// side's opposite.
    pub fn peer_is_primary(&self) -> bool {
        self.is_secondary()
    }

    pub fn set_role(&self, role: InputRole) {
        self.is_primary
            .store(matches!(role, InputRole::Primary), Ordering::SeqCst);
    }

    /// Call before sending a locally-initiated `SwitchOwnership` — bumps
    /// and returns the new generation to embed in the outgoing message.
    pub fn bump_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Call on receiving a `SwitchOwnership`: applies `incoming` iff it
    /// is strictly greater than the last generation this side accepted,
    /// atomically advancing to it. `false` means the message was a
    /// stale retransmit or an exact duplicate and the caller must not
    /// apply anything else from it (role, forwarding, suppression).
    pub fn try_advance_generation(&self, incoming: u64) -> bool {
        loop {
            let current = self.generation.load(Ordering::SeqCst);
            if incoming <= current {
                return false;
            }
            if self
                .generation
                .compare_exchange(current, incoming, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_primary_with_generation_zero() {
        let handle = OwnershipHandle::new();
        assert!(handle.is_primary());
        assert!(!handle.is_secondary());
        assert!(!handle.peer_is_primary());
        assert_eq!(handle.role(), InputRole::Primary);
    }

    #[test]
    fn set_role_flips_primary_and_secondary_together_with_peer_is_primary() {
        let handle = OwnershipHandle::new();
        handle.set_role(InputRole::Secondary);
        assert!(handle.is_secondary());
        assert!(handle.peer_is_primary());
        assert_eq!(handle.role(), InputRole::Secondary);

        handle.set_role(InputRole::Primary);
        assert!(handle.is_primary());
        assert!(!handle.peer_is_primary());
    }

    #[test]
    fn bump_generation_increments_from_one() {
        let handle = OwnershipHandle::new();
        assert_eq!(handle.bump_generation(), 1);
        assert_eq!(handle.bump_generation(), 2);
        assert_eq!(handle.bump_generation(), 3);
    }

    #[test]
    fn try_advance_generation_accepts_strictly_greater_values() {
        let handle = OwnershipHandle::new();
        assert!(handle.try_advance_generation(1));
        assert!(handle.try_advance_generation(2));
    }

    #[test]
    fn try_advance_generation_rejects_an_exact_duplicate() {
        let handle = OwnershipHandle::new();
        assert!(handle.try_advance_generation(5));
        assert!(!handle.try_advance_generation(5));
    }

    #[test]
    fn try_advance_generation_rejects_a_stale_lower_value() {
        let handle = OwnershipHandle::new();
        assert!(handle.try_advance_generation(5));
        assert!(!handle.try_advance_generation(3));
        // Confirms rejection didn't corrupt state: a later, genuinely
        // higher generation still applies.
        assert!(handle.try_advance_generation(6));
    }

    #[test]
    fn clones_share_the_same_underlying_state() {
        let handle = OwnershipHandle::new();
        let clone = handle.clone();
        clone.set_role(InputRole::Secondary);
        assert!(
            handle.is_secondary(),
            "clone shares state with the original"
        );
        assert!(handle.try_advance_generation(1));
        assert!(
            !clone.try_advance_generation(1),
            "clone shares the generation counter too"
        );
    }
}
