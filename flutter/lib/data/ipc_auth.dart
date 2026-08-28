import 'dart:io';

/// Path to the IPC auth token file `flow-daemon` writes on first run
/// (`daemon/src/ipc/auth.rs`) — `~/.flow/ipc.token`. Computed
/// independently here rather than shared via a Rust-side constant, since
/// Dart can't import one; deliberately a plain home-relative dotfile
/// (not `flow-daemon`'s own per-OS app-data directory, which would need
/// this package to reimplement the `directories` crate's per-OS layout
/// rules just to find one file) specifically so both sides can compute
/// the identical path independently.
/// `--dart-define=FLOW_IPC_TOKEN_PATH=<path>` overrides the token
/// location wholesale, matching the daemon's own `FLOW_IPC_TOKEN_PATH`
/// env var. Empty (the default) means "use the standard dotfile path".
/// Needed to point a second Flutter instance at a second local daemon's
/// separate token file.
const String _envTokenPath = String.fromEnvironment('FLOW_IPC_TOKEN_PATH');

String ipcTokenPath() {
  if (_envTokenPath.isNotEmpty) {
    return _envTokenPath;
  }
  // Windows resolves the home directory from USERPROFILE; every other
  // platform from HOME. The daemon's `ipc::auth::home_dir` picks the
  // same variable per-platform, so both sides land on the identical
  // path — checking HOME first on Windows instead would pick up the
  // MSYS-style `/c/Users/...` value a Git-Bash-launched process
  // inherits, which the daemon never sees.
  final env = Platform.environment;
  final home = Platform.isWindows
      ? (env['USERPROFILE'] ?? env['HOME'])
      : (env['HOME'] ?? env['USERPROFILE']);
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
