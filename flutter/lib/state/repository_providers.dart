import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../data/mock_daemon_repository.dart';
import '../domain/daemon_link_state.dart';
import '../domain/daemon_repository.dart';
import '../domain/device.dart';
import '../domain/pairing.dart';
import '../domain/permission_status.dart';
import '../domain/settings.dart';

/// The single source of daemon state for the whole app. Every screen
/// reads through this provider (or the `watch*Provider`s below) and
/// nothing constructs [MockDaemonRepository] itself — swapping in a real
/// IPC-backed [DaemonRepository] later means overriding this one
/// provider, not touching UI code.
final daemonRepositoryProvider = Provider<DaemonRepository>((ref) {
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
