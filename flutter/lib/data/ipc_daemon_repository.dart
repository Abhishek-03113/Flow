import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart' show debugPrint, visibleForTesting;
import 'package:stream_channel/stream_channel.dart';
import 'package:web_socket_channel/web_socket_channel.dart';

import '../domain/daemon_command_exception.dart';
import '../domain/daemon_link_state.dart';
import '../domain/daemon_repository.dart';
import '../domain/device.dart';
import '../domain/pairing.dart';
import '../domain/permission_status.dart';
import '../domain/settings.dart';
import '../domain/switch_key_binding.dart';
import 'ipc_auth.dart';
import 'ipc_codec.dart';
import 'ipc_constants.dart';
import 'replay_channel.dart';

/// How long to wait after a dropped/failed connection before dialing
/// `flow-daemon` again. `flutter run` reaches its first connection
/// attempt in well under a second; `cargo run -p flow-daemon` from a
/// clean build can take 10-20+ seconds to compile — a real, common race
/// on a fresh checkout, not an edge case. Without a retry, that race
/// permanently strands every screen that depends on daemon state in
/// whatever error the first attempt produced, even though the daemon
/// comes up and stays up moments later.
const _reconnectDelay = Duration(seconds: 1);

/// [DaemonRepository] backed by a real `flow-daemon` process over the
/// local WebSocket IPC contract (`docs/contracts/daemon-ipc.md`). A
/// drop-in replacement for [MockDaemonRepository] behind the same
/// interface — see `docs/contracts/README.md` ground rule 2. The UI never
/// imports this class directly, only through `daemonRepositoryProvider`.
///
/// Connects on construction and automatically redials on disconnect
/// (see [_reconnectDelay]); [dispose] stops that and closes the socket.
class IpcDaemonRepository implements DaemonRepository {
  /// Connects to `flow-daemon` at [uri] (defaults to
  /// [flowDaemonIpcUri]), presenting [loadIpcToken]'s value as the
  /// WebSocket subprotocol — `daemon/src/ipc/server.rs` rejects the
  /// handshake outright without a matching one (`127.0.0.1` is reachable
  /// by any local process, not just this app). A `null` token (the
  /// daemon has never run, so it hasn't generated one yet) is passed
  /// through as no protocols at all, which the daemon likewise rejects —
  /// the same "can't connect" failure shape as the daemon simply not
  /// being up. Reconnects on its own thereafter — see [_scheduleReconnect].
  IpcDaemonRepository({Uri? uri}) : _uri = uri ?? flowDaemonIpcUri() {
    _connect();
  }

  /// Drives the repository over an already-constructed channel instead of
  /// a real WebSocket — e.g. an in-memory `StreamChannelController` pair
  /// in tests, so the event-parsing/replay logic can be exercised without
  /// a live `flow-daemon` process. Never reconnects: a fixed test channel
  /// has no URI to redial, and every existing test drives exactly one
  /// channel's lifecycle on its own terms.
  @visibleForTesting
  IpcDaemonRepository.withChannel(StreamChannel<dynamic> channel)
    : _uri = null {
    _bind(channel);
  }

  /// `null` only for [withChannel] — the signal that [_handleDone] should
  /// never schedule a reconnect.
  final Uri? _uri;

  StreamChannel<dynamic>? _channel;
  StreamSubscription<dynamic>? _subscription;
  Timer? _reconnectTimer;
  bool _disposed = false;

  /// Pending command replies, keyed by request id — resolved by
  /// [_handleMessage] when the matching ack/err frame arrives (track D3).
  final _pending = <String, Completer<void>>{};

  final _devices = ReplayChannel<List<Device>>();
  final _linkState = ReplayChannel<DaemonLinkState>();
  final _pairingSession = ReplayChannel<PairingSession>();
  final _settings = ReplayChannel<FlowSettings>();
  final _permission = ReplayChannel<PermissionStatus>();

  int _nextRequestId = 0;

  void _connect() {
    final token = loadIpcToken();
    _bind(
      WebSocketChannel.connect(
        _uri!,
        protocols: token == null ? null : [token],
      ),
    );
  }

  void _bind(StreamChannel<dynamic> channel) {
    _channel = channel;
    _subscription = channel.stream.listen(
      _handleMessage,
      onError: _handleTransportError,
      onDone: _handleDone,
    );
    // `WebSocketChannel.connect` fails asynchronously via two separate
    // paths: the stream error above, and `WebSocketChannel.ready`
    // completing with the same error. Nothing else in this class ever
    // reads `ready`, so without a handler here a connection failure (e.g.
    // no `flow-daemon` process listening yet) surfaces as an *unhandled*
    // isolate error that takes the whole app down instead of landing in
    // `_handleTransportError` — this is what turns "the daemon isn't
    // running" into a hard crash. [withChannel] is sometimes driven by a
    // plain `StreamChannel` (an in-memory controller pair) that has no
    // `ready` future at all, hence the type check rather than assuming
    // `WebSocketChannel`.
    if (channel is WebSocketChannel) {
      unawaited(
        channel.ready.catchError((Object error, StackTrace stackTrace) {
          _handleTransportError(error, stackTrace);
        }),
      );
    }
  }

  void _handleMessage(dynamic data) {
    final json = jsonDecode(data as String) as Map<String, dynamic>;
    final event = json['event'];
    if (event is String) {
      _handleEvent(event, json['payload']);
      return;
    }
    final id = json['id'];
    if (id is String) _handleReply(id, json);
  }

  /// Resolves the [Completer] a command's `Future` is waiting on, by the
  /// `id` echoed back on its ack/err frame — `daemon-ipc.md`: "`id` on a
  /// command is generated by the UI and echoed back in exactly one ack;
  /// it's how the UI resolves the right `Future`".
  void _handleReply(String id, Map<String, dynamic> json) {
    final completer = _pending.remove(id);
    if (completer == null || completer.isCompleted) return;

    if (json['ok'] == true) {
      completer.complete();
      return;
    }
    final error = json['error'] as Map<String, dynamic>?;
    completer.completeError(
      DaemonCommandException(
        error?['code'] as String? ?? 'unknown_error',
        error?['message'] as String? ?? 'the daemon rejected the command',
      ),
    );
  }

  /// Routes one `event` frame by name into its matching [ReplayChannel]
  /// — the Dart-side realization of `daemon-ipc.md`'s "every `watch*`
  /// stream corresponds to one event name" rule.
  void _handleEvent(String event, dynamic payload) {
    switch (event) {
      case 'devices_changed':
        _devices.emit(devicesFromJson(payload));
      case 'link_state_changed':
        _linkState.emit(daemonLinkStateFromJson(payload as String));
      case 'pairing_session_changed':
        _pairingSession.emit(
          pairingSessionFromJson(payload as Map<String, dynamic>),
        );
      case 'settings_changed':
        _settings.emit(flowSettingsFromJson(payload as Map<String, dynamic>));
      case 'permission_changed':
        _permission.emit(
          permissionStatusFromJson(payload as Map<String, dynamic>),
        );
    }
  }

  void _handleTransportError(Object error, StackTrace stackTrace) {
    debugPrint('flow-daemon connection error: $error');
    // A transport-level error precedes the socket closing; _handleDone
    // is what resolves any still-pending commands and schedules a
    // reconnect. What's missing without this is any signal to the UI at
    // all: every `watch*` stream just sits in `AsyncLoading` forever when
    // `flow-daemon` isn't reachable yet (onboarding's permission step, for
    // one, has no way to distinguish "still connecting" from "never going
    // to connect"). Only channels that never got a real value are pushed
    // into `AsyncError` — one that already has state from a working
    // connection keeps showing it rather than being clobbered by a
    // transient drop.
    _failChannelsAwaitingFirstValue(error, stackTrace);
  }

  void _failChannelsAwaitingFirstValue(Object error, StackTrace stackTrace) {
    for (final channel in [
      _devices,
      _linkState,
      _pairingSession,
      _settings,
      _permission,
    ]) {
      if (!channel.hasValue) channel.emitError(error, stackTrace);
    }
  }

  void _handleDone() {
    // Drop the dead channel immediately rather than leaving `_sendCommand`
    // writing into a closed sink (which either throws or, worse, just
    // silently swallows the write and leaves that command's `Future`
    // hanging forever) during the gap before `_scheduleReconnect` dials
    // again.
    _channel = null;
    _subscription = null;
    final pending = List<Completer<void>>.of(_pending.values);
    _pending.clear();
    for (final completer in pending) {
      if (!completer.isCompleted) {
        completer.completeError(
          const DaemonCommandException(
            'daemon_disconnected',
            'lost connection to flow-daemon',
          ),
        );
      }
    }
    // The socket can close without ever raising a transport error first
    // (e.g. the daemon refuses the handshake outright) — same "never
    // resolve" trap as `_handleTransportError` if a channel still hasn't
    // seen its first value.
    _failChannelsAwaitingFirstValue(
      const DaemonCommandException(
        'daemon_disconnected',
        'lost connection to flow-daemon',
      ),
      StackTrace.current,
    );
    _scheduleReconnect();
  }

  /// Redials `flow-daemon` after [_reconnectDelay] — covers both a dead
  /// connection dropping later and, just as commonly during local dev, the
  /// very first attempt losing a startup race against a `flow-daemon`
  /// that's still compiling. A no-op for [withChannel] (`_uri == null`)
  /// and after [dispose].
  void _scheduleReconnect() {
    if (_uri == null || _disposed) return;
    _reconnectTimer?.cancel();
    _reconnectTimer = Timer(_reconnectDelay, () {
      if (_disposed) return;
      _connect();
    });
  }

  @override
  Stream<List<Device>> watchDevices() => _devices.watch();

  @override
  Stream<DaemonLinkState> watchLinkState() => _linkState.watch();

  @override
  Stream<PairingSession> watchPairingSession() => _pairingSession.watch();

  @override
  Stream<FlowSettings> watchSettings() => _settings.watch();

  @override
  Stream<PermissionStatus> watchPermission() => _permission.watch();

  @override
  Future<void> switchActiveDevice(String deviceId) =>
      _sendCommand('switch_active_device', {'device_id': deviceId});

  @override
  Future<void> removeDevice(String deviceId) =>
      _sendCommand('remove_device', {'device_id': deviceId});

  @override
  Future<void> startPairing() => _sendCommand('start_pairing', null);

  @override
  Future<void> cancelPairing() => _sendCommand('cancel_pairing', null);

  @override
  Future<void> pairWithCandidate(String candidateId) =>
      _sendCommand('pair_with_candidate', {'candidate_id': candidateId});

  @override
  Future<void> setSwitchKey(SwitchKeyBinding binding) =>
      _sendCommand('set_switch_key', switchKeyBindingToJson(binding));

  @override
  Future<void> updateSettings(SettingsPatch patch) =>
      _sendCommand('update_settings', settingsPatchToJson(patch));

  @override
  Future<void> resetSettings() => _sendCommand('reset_settings', null);

  @override
  Future<void> requestPermission() => _sendCommand('request_permission', null);

  @override
  Future<void> retryConnection() => _sendCommand('retry_connection', null);

  /// Sends one `IpcRequest` with a fresh id and returns a `Future` that
  /// resolves on its ack or throws [DaemonCommandException] on its err —
  /// [_handleReply] is what actually completes it, keyed by that id.
  /// Rejects immediately, without touching the (possibly absent) channel,
  /// while a reconnect is in flight — there's nothing to send it on.
  Future<void> _sendCommand(String command, dynamic payload) {
    final channel = _channel;
    if (channel == null) {
      return Future.error(
        const DaemonCommandException(
          'daemon_disconnected',
          "not connected to flow-daemon — it's either not running yet or "
              'was just restarted; retrying automatically',
        ),
      );
    }
    final id = 'req-${_nextRequestId++}';
    final completer = Completer<void>();
    _pending[id] = completer;
    channel.sink.add(
      jsonEncode({'id': id, 'command': command, 'payload': payload}),
    );
    return completer.future;
  }

  /// Releases resources — stops reconnecting and closes the socket and
  /// its subscription. Not part of [DaemonRepository]; mirrors
  /// [MockDaemonRepository.dispose].
  Future<void> dispose() async {
    _disposed = true;
    _reconnectTimer?.cancel();
    await _subscription?.cancel();
    _devices.close();
    _linkState.close();
    _pairingSession.close();
    _settings.close();
    _permission.close();
    await _channel?.sink.close();
  }
}
