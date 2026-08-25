import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart' show visibleForTesting;
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
import 'ipc_codec.dart';
import 'ipc_constants.dart';
import 'replay_channel.dart';

/// [DaemonRepository] backed by a real `flow-daemon` process over the
/// local WebSocket IPC contract (`docs/contracts/daemon-ipc.md`). A
/// drop-in replacement for [MockDaemonRepository] behind the same
/// interface — see `docs/contracts/README.md` ground rule 2. The UI never
/// imports this class directly, only through `daemonRepositoryProvider`.
///
/// Connects on construction; [dispose] closes the socket.
class IpcDaemonRepository implements DaemonRepository {
  /// Connects to `flow-daemon` at [uri] (defaults to
  /// [flowDaemonIpcUri]).
  factory IpcDaemonRepository({Uri? uri}) {
    return IpcDaemonRepository.withChannel(
      WebSocketChannel.connect(uri ?? flowDaemonIpcUri()),
    );
  }

  /// Drives the repository over an already-constructed channel instead of
  /// a real WebSocket — e.g. an in-memory `StreamChannelController` pair
  /// in tests, so the event-parsing/replay logic can be exercised without
  /// a live `flow-daemon` process.
  @visibleForTesting
  IpcDaemonRepository.withChannel(this._channel) {
    _subscription = _channel.stream.listen(
      _handleMessage,
      onError: _handleTransportError,
      onDone: _handleDone,
    );
  }

  final StreamChannel<dynamic> _channel;
  late final StreamSubscription<dynamic> _subscription;

  /// Pending command replies, keyed by request id — resolved by
  /// [_handleMessage] when the matching ack/err frame arrives (track D3).
  final _pending = <String, Completer<void>>{};

  final _devices = ReplayChannel<List<Device>>();
  final _linkState = ReplayChannel<DaemonLinkState>();
  final _pairingSession = ReplayChannel<PairingSession>();
  final _settings = ReplayChannel<FlowSettings>();
  final _permission = ReplayChannel<PermissionStatus>();

  void _handleMessage(dynamic data) {
    final json = jsonDecode(data as String) as Map<String, dynamic>;
    final event = json['event'];
    if (event is String) {
      _handleEvent(event, json['payload']);
      return;
    }
    // Ack/err resolution by request id lands in track D3.
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
    // A transport-level error precedes the socket closing; _handleDone
    // is what actually resolves any still-pending commands, so there is
    // nothing else to do here yet.
  }

  void _handleDone() {
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
      throw UnimplementedError('IPC commands are implemented in track D3');

  @override
  Future<void> removeDevice(String deviceId) =>
      throw UnimplementedError('IPC commands are implemented in track D3');

  @override
  Future<void> startPairing() =>
      throw UnimplementedError('IPC commands are implemented in track D3');

  @override
  Future<void> cancelPairing() =>
      throw UnimplementedError('IPC commands are implemented in track D3');

  @override
  Future<void> pairWithCandidate(String candidateId) =>
      throw UnimplementedError('IPC commands are implemented in track D3');

  @override
  Future<void> setSwitchKey(SwitchKeyBinding binding) =>
      throw UnimplementedError('IPC commands are implemented in track D3');

  @override
  Future<void> updateSettings(SettingsPatch patch) =>
      throw UnimplementedError('IPC commands are implemented in track D3');

  @override
  Future<void> resetSettings() =>
      throw UnimplementedError('IPC commands are implemented in track D3');

  @override
  Future<void> requestPermission() =>
      throw UnimplementedError('IPC commands are implemented in track D3');

  /// Releases resources — closes the socket and its subscription. Not
  /// part of [DaemonRepository]; mirrors [MockDaemonRepository.dispose].
  Future<void> dispose() async {
    await _subscription.cancel();
    _devices.close();
    _linkState.close();
    _pairingSession.close();
    _settings.close();
    _permission.close();
    await _channel.sink.close();
  }
}
