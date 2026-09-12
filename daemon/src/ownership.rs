//! The session-local ownership state a paired connection's pipeline and
//! the switch-key runner both need: this daemon's own [`InputRole`] and
//! the monotonic generation guarding `ChannelMessage::OwnershipChanged`
//! against stale/duplicate delivery ("Complete Flow V1" task §5's
//! idempotent ownership update: "incoming generation <= current
//! generation -> ignore") *and*, since the "Fix Flow V1 Ownership
//! Synchronization" pass, against split-brain on reconnect: see
//! [`Self::reconcile`] and `pipeline::resolve_ownership`.
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

    /// Call before sending a locally-initiated `OwnershipChanged` — bumps
    /// and returns the new generation to embed in the outgoing message.
    pub fn bump_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Call on receiving a live `OwnershipChanged` handoff: applies
    /// `incoming` iff it is strictly greater than the last generation this
    /// side accepted, atomically advancing to it. `false` means the
    /// message was a stale retransmit or an exact duplicate and the caller
    /// must not apply anything else from it (role, forwarding, suppression).
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

    /// This side's current generation, for building the handshake message
    /// a new connection opens with (`pipeline::run_paired_connection`).
    pub fn generation_now(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Applies the outcome of `pipeline::resolve_ownership` — the
    /// connection-opening handshake that reconciles this side's belief
    /// with the peer's — unconditionally, unlike [`Self::try_advance_generation`].
    /// This is deliberately *not* gated the same way: the handshake already
    /// picked the one deterministic answer both sides independently agree
    /// on, so there is nothing left to arbitrate here, only to apply.
    ///
    /// The generation floor is raised to `max(current, resolved_generation)`
    /// — never lowered — so a stale live handoff from before the reconnect
    /// still can't re-apply afterward. Returns whether the role actually
    /// changed (for logging).
    pub fn reconcile(&self, resolved_primary_is_local: bool, resolved_generation: u64) -> bool {
        let changed = self.is_primary() != resolved_primary_is_local;
        self.is_primary
            .store(resolved_primary_is_local, Ordering::SeqCst);

        let mut current = self.generation.load(Ordering::SeqCst);
        while resolved_generation > current {
            match self.generation.compare_exchange(
                current,
                resolved_generation,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
        changed
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
    fn generation_now_reads_without_mutating() {
        let handle = OwnershipHandle::new();
        assert_eq!(handle.generation_now(), 0);
        handle.bump_generation();
        assert_eq!(handle.generation_now(), 1);
        assert_eq!(
            handle.generation_now(),
            1,
            "reading again doesn't advance it"
        );
    }

    #[test]
    fn reconcile_applies_the_resolved_role_and_raises_the_generation_floor() {
        let handle = OwnershipHandle::new();
        assert!(handle.is_primary());

        let changed = handle.reconcile(false, 5);
        assert!(changed, "role actually flipped");
        assert!(handle.is_secondary());
        assert_eq!(handle.generation_now(), 5);
    }

    #[test]
    fn reconcile_to_the_same_role_reports_no_change() {
        let handle = OwnershipHandle::new();
        let changed = handle.reconcile(true, 3);
        assert!(!changed, "already Primary; nothing flipped");
        assert_eq!(handle.generation_now(), 3);
    }

    #[test]
    fn reconcile_never_lowers_the_generation_floor() {
        let handle = OwnershipHandle::new();
        handle.bump_generation(); // generation 1
        handle.reconcile(false, 0);
        assert_eq!(
            handle.generation_now(),
            1,
            "a lower resolved generation must not erase a higher one already seen"
        );
        assert!(
            handle.is_secondary(),
            "the resolved role still applies even when the generation floor doesn't move"
        );
    }

    #[test]
    fn a_stale_live_handoff_cannot_reapply_after_a_reconcile_raised_the_floor() {
        let handle = OwnershipHandle::new();
        handle.reconcile(false, 10);
        // A live handoff at a generation below the reconciled floor must be
        // rejected exactly like any other stale message.
        assert!(!handle.try_advance_generation(4));
        assert!(handle.try_advance_generation(11));
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
