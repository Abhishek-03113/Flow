//! Health of this machine's connection to the daemon (`data-model.md`
//! "DaemonLinkState"). A single top-level value, not per-device — it
//! drives the tray popover's status dot and banner.

/// Reachability/health of the daemon connection, distinct from any single
/// device's [`crate::device::DeviceState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
