import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../data/ipc_daemon_repository.dart';
import '../data/mock_daemon_repository.dart';
import '../domain/daemon_link_state.dart';
import '../domain/daemon_repository.dart';
import '../domain/device.dart';
import '../domain/pairing.dart';
import '../domain/permission_status.dart';
import '../domain/settings.dart';

/// Selects the daemon backend at build time via
/// `--dart-define=FLOW_DAEMON_MODE=ipc` (see `flutter/README.md`).
/// Defaults to `mock` — unset, `flutter run` behaves exactly as before,
/// so this is not a regression for UI-only development.
const _daemonMode = String.fromEnvironment(
  'FLOW_DAEMON_MODE',
  defaultValue: 'mock',
);

/// The single source of daemon state for the whole app. Every screen
/// reads through this provider (or the `watch*Provider`s below) and
/// nothing constructs [MockDaemonRepository] or [IpcDaemonRepository]
/// itself — the mock stays the default for UI-only development;
/// `FLOW_DAEMON_MODE=ipc` opts into a real `flow-daemon` process at
/// `127.0.0.1:47823` for daemon-integration testing.
final daemonRepositoryProvider = Provider<DaemonRepository>((ref) {
  if (_daemonMode == 'ipc') {
    final repository = IpcDaemonRepository();
    ref.onDispose(() => unawaited(repository.dispose()));
    return repository;
  }
  final repository = MockDaemonRepository();
  ref.onDispose(repository.dispose);
  return repository;
});

final devicesProvider = StreamProvider<List<Device>>((ref) {
  return ref.watch(daemonRepositoryProvider).watchDevices();
});

final linkStateProvider = StreamProvider<DaemonLinkState>((ref) {
  return ref.watch(daemonRepositoryProvider).watchLinkState();
});

final pairingSessionProvider = StreamProvider<PairingSession>((ref) {
  return ref.watch(daemonRepositoryProvider).watchPairingSession();
});

final settingsProvider = StreamProvider<FlowSettings>((ref) {
  return ref.watch(daemonRepositoryProvider).watchSettings();
});

final permissionProvider = StreamProvider<PermissionStatus>((ref) {
  return ref.watch(daemonRepositoryProvider).watchPermission();
});
