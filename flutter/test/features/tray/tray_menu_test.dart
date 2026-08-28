import 'package:flow_ui/domain/daemon_link_state.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/features/tray/tray_menu.dart';
import 'package:flutter_test/flutter_test.dart';

Device _dev(String id, String name, DeviceState state) => Device(
  id: id,
  name: name,
  os: HostOs.macos,
  state: state,
  lastSeen: DateTime(2026),
);

void main() {
  test('connected: status, active, switch target, disconnected, menu rows', () {
    final entries = buildTrayMenu(
      link: DaemonLinkState.connected,
      localDeviceId: 'd1',
      devices: [
        _dev('d1', 'MacBook', DeviceState.active),
        _dev('d2', 'Work Laptop', DeviceState.inactive),
        _dev('d3', 'Desktop', DeviceState.disconnected),
      ],
    );
    final labels = entries
        .where((e) => !e.isSeparator)
        .map((e) => e.label)
        .toList();
    expect(labels, [
      'Flow — Connected',
      'Using: MacBook (macOS)',
      'Switch to Work Laptop',
      'Desktop — disconnected',
      'Pair New Device…',
      'Dashboard',
      'Settings',
      'Quit Flow',
    ]);

    final switchRow = entries.firstWhere(
      (e) => e.label == 'Switch to Work Laptop',
    );
    expect(switchRow.enabled, isTrue);
    expect(switchRow.action, const TrayAction.switchDevice('d2'));

    final disc = entries.firstWhere((e) => e.label == 'Desktop — disconnected');
    expect(disc.enabled, isFalse);

    expect(
      entries.firstWhere((e) => e.label == 'Flow — Connected').enabled,
      isFalse,
    );
  });

  test('no active device: no "Using:" row', () {
    final entries = buildTrayMenu(
      link: DaemonLinkState.connecting,
      localDeviceId: 'd1',
      devices: [_dev('d2', 'Work Laptop', DeviceState.inactive)],
    );
    expect(entries.any((e) => e.label.startsWith('Using:')), isFalse);
    expect(entries.first.label, 'Flow — Connecting…');
  });

  test('permissionRequired status label', () {
    final entries = buildTrayMenu(
      link: DaemonLinkState.permissionRequired,
      localDeviceId: 'd1',
      devices: const [],
    );
    expect(entries.first.label, 'Flow — Needs permission');
  });
}
