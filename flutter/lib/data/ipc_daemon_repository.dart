import 'dart:async';

import 'package:web_socket_channel/web_socket_channel.dart';

import '../domain/daemon_command_exception.dart';
import '../domain/daemon_link_state.dart';
import '../domain/daemon_repository.dart';
import '../domain/device.dart';
import '../domain/pairing.dart';
import '../domain/permission_status.dart';
import '../domain/settings.dart';
import '../domain/switch_key_binding.dart';
import 'ipc_constants.dart';
import 'replay_channel.dart';

/// [DaemonRepository] backed by a real `flow-daemon` process over the
/// local WebSocket IPC contract (`docs/contracts/daemon-ipc.md`). A
/// drop-in replacement for [MockDaemonRepository] behind the same
/// interface — see `docs/contracts/README.md` ground rule 2. The UI never
/// imports this class directly, only through `daemonRepositoryProvider`.
///
/// Connects on construction; [dispose] closes the socket. Message parsing
/// (state events -> the five [watch] streams) is track D2's job, and the
/// nine commands are track D3's — this class is the connection-lifecycle
/// skeleton both build on.
class IpcDaemonRepository implements DaemonRepository {
  IpcDaemonRepository({Uri? uri})
    : _channel = WebSocketChannel.connect(uri ?? flowDaemonIpcUri()) {
    _subscription = _channel.stream.listen(
      _handleMessage,
      onError: _handleTransportError,
      onDone: _handleDone,
    );
  }

  final WebSocketChannel _channel;
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
    // Event frame parsing (D2) and ack/err resolution (D3) land here.
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
