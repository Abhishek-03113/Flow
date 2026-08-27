import 'dart:async';

import '../domain/daemon_command_exception.dart';
import '../domain/daemon_link_state.dart';
import '../domain/daemon_repository.dart';
import '../domain/device.dart';
import '../domain/pairing.dart';
import '../domain/permission_status.dart';
import '../domain/settings.dart';
import '../domain/switch_key_binding.dart';
import 'replay_channel.dart';

/// In-memory stand-in for the daemon, implementing [DaemonRepository]
/// exactly as specified in `docs/contracts/daemon-ipc.md`. This is the
/// only implementation that exists until a real daemon and an IPC
/// transport do; a future `IpcDaemonRepository` is a drop-in replacement
/// behind the same interface.
///
/// The pairing/switching *sequences* here (searching -> found ->
/// requesting -> paired -> idle, and the switch debounce) are part of the
/// contract. The exact millisecond delays are not — they stand in for
/// real network/human latency a real daemon won't have on a fixed
/// schedule.
class MockDaemonRepository implements DaemonRepository {
  MockDaemonRepository() {
    _devices = ReplayChannel<List<Device>>(_seedDevices());
    _linkState = ReplayChannel(DaemonLinkState.connected);
    _pairingSession = ReplayChannel(PairingSession.idle);
    _settings = ReplayChannel(FlowSettings.defaults());
    _permission = ReplayChannel(
      const PermissionStatus(name: 'Accessibility access', granted: false),
    );
  }

  /// The local machine — never removable, never a pairing candidate.
  static const _localDeviceId = 'd1';

  static const _candidateSeeds = [
    PairingCandidate(
      id: 'cand-office-mini',
      name: 'Office Mac Mini',
      os: HostOs.macos,
    ),
    PairingCandidate(
      id: 'cand-studio-linux',
      name: 'Studio Linux',
      os: HostOs.linux,
    ),
  ];

  static const _searchDuration = Duration(milliseconds: 1200);
  static const _pairRequestDuration = Duration(milliseconds: 1500);
  static const _pairedAutoIdleDuration = Duration(milliseconds: 1600);
  static const _switchDebounce = Duration(milliseconds: 400);
  static const _retryConnectDuration = Duration(milliseconds: 900);

  late final ReplayChannel<List<Device>> _devices;
  late final ReplayChannel<DaemonLinkState> _linkState;
  late final ReplayChannel<PairingSession> _pairingSession;
  late final ReplayChannel<FlowSettings> _settings;
  late final ReplayChannel<PermissionStatus> _permission;

  final _timers = <Timer>[];

  static List<Device> _seedDevices() {
    final now = DateTime.now();
    return [
      Device(
        id: 'd1',
        name: 'MacBook',
        os: HostOs.macos,
        state: DeviceState.active,
        lastSeen: now,
      ),
      Device(
        id: 'd2',
        name: 'Work Laptop',
        os: HostOs.windows,
        state: DeviceState.inactive,
        lastSeen: now.subtract(const Duration(minutes: 2)),
      ),
      Device(
        id: 'd3',
        name: 'Desktop',
        os: HostOs.linux,
        state: DeviceState.disconnected,
        lastSeen: now.subtract(const Duration(days: 3)),
      ),
    ];
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

  /// Exposed for the dev harness (`todos.json` task S2) to force any
  /// [DaemonLinkState] for visual QA of every banner variant. Not part of
  /// [DaemonRepository] — a real daemon reports its own link state, it
  /// isn't told what to report.
  void debugSetLinkState(DaemonLinkState state) => _linkState.emit(state);

  @override
  Future<void> switchActiveDevice(String deviceId) async {
    final target = _deviceOrThrow(deviceId);
    if (target.state != DeviceState.inactive &&
        target.state != DeviceState.connected) {
      throw DaemonCommandException(
        'device_not_switchable',
        '${target.name} is ${target.state.name}, not inactive or connected',
      );
    }
    await _delay(_switchDebounce);
    _devices.emit([
      for (final d in _devices.value)
        if (d.id == deviceId)
          d.copyWith(state: DeviceState.active, lastSeen: DateTime.now())
        else if (d.state == DeviceState.active)
          d.copyWith(state: DeviceState.inactive)
        else
          d,
    ]);
  }

  @override
  Future<void> removeDevice(String deviceId) async {
    if (deviceId == _localDeviceId) {
      throw const DaemonCommandException(
        'device_not_removable',
        'the local device cannot be removed',
      );
    }
    _deviceOrThrow(deviceId);
    _devices.emit(_devices.value.where((d) => d.id != deviceId).toList());
  }

  @override
  Future<void> startPairing() async {
    if (_pairingSession.value.stage != PairingStage.idle) {
      throw const DaemonCommandException(
        'pairing_in_progress',
        'a pairing session is already active',
      );
    }
    _pairingSession.emit(const PairingSession(stage: PairingStage.searching));
    _later(_searchDuration, () {
      if (_pairingSession.value.stage != PairingStage.searching) return;
      final known = _devices.value.map((d) => d.name).toSet();
      final candidates = _candidateSeeds
          .where((c) => !known.contains(c.name))
          .toList();
      _pairingSession.emit(
        PairingSession(stage: PairingStage.found, candidates: candidates),
      );
    });
  }

  @override
  Future<void> cancelPairing() async {
    if (_pairingSession.value.stage == PairingStage.idle) {
      throw const DaemonCommandException(
        'pairing_not_active',
        'no pairing session to cancel',
      );
    }
    _cancelTimers();
    _pairingSession.emit(PairingSession.idle);
  }

  @override
  Future<void> pairWithCandidate(String candidateId) async {
    final session = _pairingSession.value;
    if (session.stage != PairingStage.found) {
      throw DaemonCommandException(
        'pairing_not_ready',
        'pairing session is ${session.stage.name}, not found',
      );
    }
    final candidate = session.candidates
        .where((c) => c.id == candidateId)
        .firstOrNull;
    if (candidate == null) {
      throw DaemonCommandException(
        'candidate_not_found',
        'no candidate $candidateId',
      );
    }
    _pairingSession.emit(
      session.copyWith(
        stage: PairingStage.requesting,
        targetName: candidate.name,
      ),
    );
    _later(_pairRequestDuration, () {
      if (_pairingSession.value.stage != PairingStage.requesting) return;
      _devices.emit([
        ..._devices.value,
        Device(
          id: candidate.id,
          name: candidate.name,
          os: candidate.os,
          state: DeviceState.inactive,
          lastSeen: DateTime.now(),
        ),
      ]);
      _pairingSession.emit(
        _pairingSession.value.copyWith(stage: PairingStage.paired),
      );
      _later(_pairedAutoIdleDuration, () {
        if (_pairingSession.value.stage != PairingStage.paired) return;
        _pairingSession.emit(PairingSession.idle);
      });
    });
  }

  @override
  Future<void> setSwitchKey(SwitchKeyBinding binding) async {
    if (binding.keys.isEmpty) {
      throw const DaemonCommandException(
        'invalid_switch_key',
        'binding must have at least one key',
      );
    }
    _settings.emit(
      _settings.value.applyPatch(SettingsPatch(switchKey: binding)),
    );
  }

  @override
  Future<void> updateSettings(SettingsPatch patch) async {
    _settings.emit(_settings.value.applyPatch(patch));
  }

  @override
  Future<void> resetSettings() async {
    _settings.emit(FlowSettings.defaults());
  }

  @override
  Future<void> requestPermission() async {
    if (_permission.value.granted) {
      throw const DaemonCommandException(
        'permission_already_granted',
        'permission already granted',
      );
    }
    _permission.emit(_permission.value.copyWith(granted: true));
  }

  @override
  Future<void> retryConnection() async {
    final current = _linkState.value;
    if (current != DaemonLinkState.disconnected &&
        current != DaemonLinkState.error) {
      throw DaemonCommandException(
        'link_not_recoverable',
        'link state is ${current.name}, not disconnected or error',
      );
    }
    _linkState.emit(DaemonLinkState.connecting);
    _later(_retryConnectDuration, () {
      if (_linkState.value != DaemonLinkState.connecting) return;
      _linkState.emit(DaemonLinkState.connected);
    });
  }

  Device _deviceOrThrow(String deviceId) {
    final device = _devices.value.where((d) => d.id == deviceId).firstOrNull;
    if (device == null) {
      throw DaemonCommandException('device_not_found', 'no device $deviceId');
    }
    return device;
  }

  Future<void> _delay(Duration duration) {
    final completer = Completer<void>();
    _timers.add(Timer(duration, completer.complete));
    return completer.future;
  }

  void _later(Duration duration, void Function() fn) {
    _timers.add(Timer(duration, fn));
  }

  void _cancelTimers() {
    for (final timer in _timers) {
      timer.cancel();
    }
    _timers.clear();
  }

  /// Releases resources. Not part of [DaemonRepository] — a real IPC
  /// client would have its own connection lifecycle, not a symmetrical
  /// `dispose`.
  void dispose() {
    _cancelTimers();
    _devices.close();
    _linkState.close();
    _pairingSession.close();
    _settings.close();
    _permission.close();
  }
}

extension<T> on Iterable<T> {
  T? get firstOrNull => isEmpty ? null : first;
}
