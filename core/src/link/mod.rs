//! Health of this machine's connection to the daemon (`data-model.md`
//! "DaemonLinkState"). A single top-level value, not per-device — it
//! drives the tray popover's status dot and banner.

use serde::{Deserialize, Serialize};

/// Reachability/health of the daemon connection, distinct from any single
/// device's [`crate::device::DeviceState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonLinkState {
    /// Daemon reachable, active device receiving input normally.
    /// UI treatment: green dot, no banner.
    Connected,
    /// Initial connection to the daemon still being established.
    /// UI treatment: amber dot (pulsing), no banner.
    Connecting,
    /// Was connected, lost the link, retrying automatically.
    /// UI treatment: amber dot (pulsing), banner with Cancel.
    Reconnecting,
    /// Daemon unreachable and not retrying (or the active device dropped
    /// and nothing has taken over).
    /// UI treatment: gray dot, banner with Retry.
    Disconnected,
    /// Daemon reachable but input sharing is failing for a reason other
    /// than permission (e.g. injection failing).
    /// UI treatment: red dot, banner with Retry.
    Error,
    /// The daemon needs an OS input-capture permission (macOS
    /// Accessibility, Windows input access, Linux input device access)
    /// before it can do anything.
    /// UI treatment: red dot, banner with Allow, routes to the permission
    /// step.
    PermissionRequired,
}

/// Why the last link attempt to a paired device failed: the stable strings
/// carried by `Device.link_error` (`data-model.md`). The UI has copy for
/// each one, so the engine never puts anything else there.
pub mod link_error {
    /// Both computers have the same role, or a controlled computer already
    /// has a controller. Not retried until a role changes.
    pub const ROLE_CONFLICT: &str = "role_conflict";
    /// The peer speaks another protocol version. Not retried.
    pub const PROTOCOL_MISMATCH: &str = "protocol_mismatch";
    /// The peer sent something out of order or unexpected.
    pub const PROTOCOL_ERROR: &str = "protocol_error";
    /// The peer did not say hello in time.
    pub const HELLO_TIMEOUT: &str = "hello_timeout";
    /// The connection closed before the link came up.
    pub const CONNECTION_LOST: &str = "connection_lost";
    /// This (controlled) computer could not start injecting input.
    pub const INJECTOR_UNAVAILABLE: &str = "injector_unavailable";

    /// Every code, for contract checks.
    pub const ALL: [&str; 6] = [
        ROLE_CONFLICT,
        PROTOCOL_MISMATCH,
        PROTOCOL_ERROR,
        HELLO_TIMEOUT,
        CONNECTION_LOST,
        INJECTOR_UNAVAILABLE,
    ];

    /// Maps a refusal reason received from a peer (untrusted text) onto
    /// this vocabulary; anything unknown is a protocol error.
    pub fn from_peer(reason: &str) -> &'static str {
        ALL.into_iter()
            .find(|code| *code == reason)
            .unwrap_or(PROTOCOL_ERROR)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_peer_reason_outside_the_vocabulary_becomes_a_protocol_error() {
            assert_eq!(from_peer("role_conflict"), ROLE_CONFLICT);
            assert_eq!(from_peer("<script>"), PROTOCOL_ERROR);
            assert_eq!(from_peer(""), PROTOCOL_ERROR);
        }
    }
}
