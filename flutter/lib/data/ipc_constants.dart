/// Local IPC transport constants (`docs/contracts/daemon-ipc.md`).
///
/// [kFlowDaemonIpcPort] must be kept in sync by hand with
/// `flow_core::ipc::IPC_PORT` (`core/src/ipc/mod.rs`) — Dart can't import
/// a Rust `const` directly. A drift between the two isn't a silent
/// failure mode: [IpcDaemonRepository] simply can't connect, the same as
/// any other "daemon isn't running" case.
const int kFlowDaemonIpcPort = 47823;

/// The daemon's IPC WebSocket URI.
Uri flowDaemonIpcUri({int port = kFlowDaemonIpcPort}) =>
    Uri.parse('ws://127.0.0.1:$port');
