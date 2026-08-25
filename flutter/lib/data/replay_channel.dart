import 'dart:async';

/// Broadcasts a value to every listener and replays the current value (if
/// any) to each new subscriber, so [watch] doubles as the initial fetch —
/// see `docs/contracts/daemon-ipc.md`'s "connecting to the channel and
/// subscribing is the initial fetch" rule. Shared by [MockDaemonRepository]
/// (seeded synchronously at construction) and [IpcDaemonRepository] (seeded
/// asynchronously by the first matching Event frame — there is nothing to
/// replay until then, hence [value] is nullable here).
///
/// Built on [Stream.multi] rather than an `async*` generator wrapping a
/// broadcast stream: `async*` needs at least one microtask before its
/// `yield*` establishes the nested subscription, which can silently drop
/// an [emit] that lands in that gap. `Stream.multi`'s listener callback
/// runs synchronously on `.listen()`, so the live subscription is already
/// in place by the time this call returns — no dropped events.
class ReplayChannel<T> {
  ReplayChannel([this._value]);

  T? _value;
  final _controller = StreamController<T>.broadcast();

  /// The current value. Only valid once at least one value exists (set
  /// via the constructor or [emit]) — throws otherwise. [MockDaemonRepository]
  /// always seeds every channel at construction, so this is always safe
  /// there; [IpcDaemonRepository] never calls this getter, since there is
  /// no current value to read before the first matching Event frame
  /// arrives — it only ever [emit]s and [watch]es.
  T get value => _value as T;

  void emit(T value) {
    _value = value;
    _controller.add(value);
  }

  Stream<T> watch() {
    return Stream<T>.multi((controller) {
      final current = _value;
      if (current != null) controller.add(current);
      final sub = _controller.stream.listen(
        controller.add,
        onError: controller.addError,
      );
      controller.onCancel = sub.cancel;
    });
  }

  void close() => _controller.close();
}
