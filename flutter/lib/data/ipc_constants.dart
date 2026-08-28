/// Local IPC transport constants (`docs/contracts/daemon-ipc.md`).
///
/// [kFlowDaemonIpcPort] must be kept in sync by hand with
/// `flow_core::ipc::IPC_PORT` (`core/src/ipc/mod.rs`) — Dart can't import
/// a Rust `const` directly. A drift between the two isn't a silent
/// failure mode: [IpcDaemonRepository] simply can't connect, the same as
/// any other "daemon isn't running" case.
const int kFlowDaemonIpcPort = 47823;

/// The port the running `flow-daemon` actually listens on. Defaults to
/// [kFlowDaemonIpcPort]; `--dart-define=FLOW_IPC_PORT=<n>` overrides it,
/// matching the daemon's own `FLOW_IPC_PORT` env var — the two must agree
/// to run a second local instance (see `daemon/README.md`, "Running a
/// second instance").
const int kFlowDaemonIpcPortResolved = int.fromEnvironment(
  'FLOW_IPC_PORT',
  defaultValue: kFlowDaemonIpcPort,
);

/// The daemon's IPC WebSocket URI.
Uri flowDaemonIpcUri({int port = kFlowDaemonIpcPortResolved}) =>
    Uri.parse('ws://127.0.0.1:$port');
