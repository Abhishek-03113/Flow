/// Local IPC client for talking to the Flow daemon (vision.md §9,
/// Communication Between Flutter and Daemon).
///
/// The concrete transport (Unix domain socket, named pipe, or local TCP
/// fallback) is decided at implementation time; callers should only depend
/// on this interface, never on how the daemon is actually reached.
library;

abstract class DaemonClient {
  Future<void> connect();
  Future<void> disconnect();
}
