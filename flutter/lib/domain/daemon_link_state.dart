/// Health of this machine's connection to the daemon and, by extension,
/// the active input link. A single top-level value, not per-device — see
/// `docs/contracts/data-model.md`.
enum DaemonLinkState {
  connected,
  connecting,
  reconnecting,
  disconnected,
  error,
  permissionRequired,
}
