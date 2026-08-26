import 'dart:io';

/// Path to the IPC auth token file `flow-daemon` writes on first run
/// (`daemon/src/ipc/auth.rs`) — `~/.flow/ipc.token`. Computed
/// independently here rather than shared via a Rust-side constant, since
/// Dart can't import one; deliberately a plain home-relative dotfile
/// (not `flow-daemon`'s own per-OS app-data directory, which would need
/// this package to reimplement the `directories` crate's per-OS layout
/// rules just to find one file) specifically so both sides can compute
/// the identical path independently.
String ipcTokenPath() {
  final home =
      Platform.environment['HOME'] ?? Platform.environment['USERPROFILE'];
  if (home == null || home.isEmpty) {
    throw StateError("could not determine the current user's home directory");
  }
  return '$home/.flow/ipc.token';
}

/// Reads the persisted IPC auth token, or `null` if the daemon hasn't
/// generated one yet (e.g. it has never been run). A missing token is
/// treated the same as "daemon isn't reachable" by the caller, not as a
/// distinct error — [IpcDaemonRepository] simply won't be able to
/// complete a handshake without it, the same failure shape as the daemon
/// not running at all.
String? loadIpcToken() {
  final file = File(ipcTokenPath());
  if (!file.existsSync()) {
    return null;
  }
  final contents = file.readAsStringSync().trim();
  return contents.isEmpty ? null : contents;
}
