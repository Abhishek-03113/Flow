import 'dart:async';

import 'package:flutter/foundation.dart' show debugPrint;
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
/// `--dart-define=FLOW_DAEMON_MODE=mock` (see `flutter/README.md`).
/// Defaults to `ipc`: the shipped app now expects a real `flow-daemon`
/// process at `127.0.0.1:47823`, matching the shipped app's real tray/
/// window (`app.dart`) rather than the dev harness's simulated
/// platforms/connection states. UI-only development without a daemon
/// still works — pass `FLOW_DAEMON_MODE=mock` explicitly (this is also
/// what the dev harness, `--dart-define=FLOW_UI_MODE=harness`, is for).
/// `flutter test` never depends on this default either way: every widget
/// test overrides `daemonRepositoryProvider` explicitly with a
/// `MockDaemonRepository` instance rather than reading this constant.
const _daemonMode = String.fromEnvironment(
  'FLOW_DAEMON_MODE',
  defaultValue: 'ipc',
);

/// The single source of daemon state for the whole app. Every screen
/// reads through this provider (or the `watch*Provider`s below) and
/// nothing constructs [MockDaemonRepository] or [IpcDaemonRepository]
/// itself — `IpcDaemonRepository` (a real `flow-daemon` process over
/// local IPC) is the default; `FLOW_DAEMON_MODE=mock` opts into the mock
/// for UI-only development or demoing without a daemon running.
final daemonRepositoryProvider = Provider<DaemonRepository>((ref) {
  // Not a hidden UI signal — a plain terminal log for whoever is running
  // `flutter run` and trying to tell "the app is on the mock backend" apart
  // from "it's on IPC but can't reach flow-daemon", which look identical
  // in the UI otherwise but need completely different fixes.
  debugPrint(
    'flow-daemon: daemon backend = $_daemonMode'
    '${_daemonMode == 'ipc' ? '' : ' (pass --dart-define=FLOW_DAEMON_MODE=ipc for the real daemon)'}',
  );
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

final incomingPairingRequestProvider = StreamProvider<IncomingPairingRequest?>((
  ref,
) {
  return ref.watch(daemonRepositoryProvider).watchIncomingPairingRequest();
});

final settingsProvider = StreamProvider<FlowSettings>((ref) {
  return ref.watch(daemonRepositoryProvider).watchSettings();
});

final permissionProvider = StreamProvider<PermissionStatus>((ref) {
  return ref.watch(daemonRepositoryProvider).watchPermission();
});
