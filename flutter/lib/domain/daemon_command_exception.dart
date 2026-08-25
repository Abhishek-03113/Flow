/// Thrown by a [DaemonRepository] command when the daemon rejects it — the
/// Dart realization of the `{ code, message }` error shape in
/// `docs/contracts/daemon-ipc.md`. [code] is a stable, machine-readable
/// string (e.g. `device_not_found`); [message] is human-readable and not
/// meant to be matched on.
class DaemonCommandException implements Exception {
  const DaemonCommandException(this.code, this.message);

  final String code;
  final String message;

  @override
  String toString() => 'DaemonCommandException($code): $message';
}
