import 'package:flow_ui/data/mock_daemon_repository.dart';
import 'package:flow_ui/domain/daemon_link_state.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/state/repository_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

/// These tests exercise `devicesProvider`/`linkStateProvider`'s own
/// forwarding through `daemonRepositoryProvider` — not which backend the
/// app defaults to at runtime (`repository_providers.dart` now defaults
/// to `IpcDaemonRepository`, which needs a real `flow-daemon` process
/// and would leave these providers stuck in `AsyncLoading`) — so, like
/// every other test in this suite, they override the repository
/// explicitly with a `MockDaemonRepository` instance.
ProviderContainer _mockContainer() {
  return ProviderContainer(
    overrides: [
      daemonRepositoryProvider.overrideWithValue(MockDaemonRepository()),
    ],
  );
}

void main() {
  test(
    'devicesProvider streams the mock repository\'s seeded devices',
    () async {
      final container = _mockContainer();
      addTearDown(container.dispose);

      final sub = container.listen(devicesProvider, (_, _) {});
      addTearDown(sub.close);

      // The stream provider starts in AsyncLoading until its first event.
      await Future<void>.delayed(Duration.zero);
      final devices = container.read(devicesProvider).requireValue;
      expect(devices, hasLength(3));
      expect(devices.first.state, DeviceState.active);
    },
  );

  test('linkStateProvider defaults to connected', () async {
    final container = _mockContainer();
    addTearDown(container.dispose);

    final sub = container.listen(linkStateProvider, (_, _) {});
    addTearDown(sub.close);

    await Future<void>.delayed(Duration.zero);
    expect(
      container.read(linkStateProvider).requireValue,
      DaemonLinkState.connected,
    );
  });
}
