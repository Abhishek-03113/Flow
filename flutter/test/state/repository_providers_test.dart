import 'package:flow_ui/domain/daemon_link_state.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/state/repository_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test(
    'devicesProvider streams the mock repository\'s seeded devices',
    () async {
      final container = ProviderContainer();
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
    final container = ProviderContainer();
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
