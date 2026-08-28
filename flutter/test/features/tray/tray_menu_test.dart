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

  test('non-local error-state device: disabled "— unavailable" row', () {
    final entries = buildTrayMenu(
      link: DaemonLinkState.connected,
      localDeviceId: 'd1',
      devices: [_dev('d2', 'Work Laptop', DeviceState.error)],
    );
    final row = entries.firstWhere(
      (e) => e.label == 'Work Laptop — unavailable',
    );
    expect(row.enabled, isFalse);
    expect(row.action, isNull);
    expect(
      entries.any((e) => e.label == 'Work Laptop — disconnected'),
      isFalse,
    );
  });

  test('non-local pairing-state device: disabled "— pairing…" row', () {
    final entries = buildTrayMenu(
      link: DaemonLinkState.connected,
      localDeviceId: 'd1',
      devices: [_dev('d2', 'Work Laptop', DeviceState.pairing)],
    );
    final row = entries.firstWhere((e) => e.label == 'Work Laptop — pairing…');
    expect(row.enabled, isFalse);
    expect(row.action, isNull);
  });

  test('remote active over local: "Using:" row shows the remote device', () {
    final entries = buildTrayMenu(
      link: DaemonLinkState.connected,
      localDeviceId: 'd1',
      devices: [
        _dev('d1', 'MacBook', DeviceState.inactive),
        _dev('d2', 'Work Laptop', DeviceState.active),
      ],
    );
    expect(entries.any((e) => e.label == 'Using: Work Laptop (macOS)'), isTrue);
    expect(entries.any((e) => e.label.startsWith('Using: MacBook')), isFalse);
  });

  test('permissionRequired status label', () {
    final entries = buildTrayMenu(
      link: DaemonLinkState.permissionRequired,
      localDeviceId: 'd1',
      devices: const [],
    );
    expect(entries.first.label, 'Flow — Needs permission');
  });

  test('TrayAction public discriminant: kind and switchDeviceId', () {
    final entries = buildTrayMenu(
      link: DaemonLinkState.connected,
      localDeviceId: 'd1',
      devices: [
        _dev('d1', 'MacBook', DeviceState.active),
        _dev('d2', 'Work Laptop', DeviceState.inactive),
      ],
    );

    // Test switchDevice action has correct kind and deviceId
    final switchRow = entries.firstWhere(
      (e) => e.label == 'Switch to Work Laptop',
    );
    expect(switchRow.action!.kind, TrayActionKind.switchDevice);
    expect(switchRow.action!.switchDeviceId, 'd2');

    // Test other actions have correct kind and null switchDeviceId
    final dashboardRow = entries.firstWhere((e) => e.label == 'Dashboard');
    expect(dashboardRow.action!.kind, TrayActionKind.openDashboard);
    expect(dashboardRow.action!.switchDeviceId, isNull);

    final pairRow = entries.firstWhere((e) => e.label == 'Pair New Device…');
    expect(pairRow.action!.kind, TrayActionKind.pairNewDevice);
    expect(pairRow.action!.switchDeviceId, isNull);

    final settingsRow = entries.firstWhere((e) => e.label == 'Settings');
    expect(settingsRow.action!.kind, TrayActionKind.openSettings);
    expect(settingsRow.action!.switchDeviceId, isNull);

    final quitRow = entries.firstWhere((e) => e.label == 'Quit Flow');
    expect(quitRow.action!.kind, TrayActionKind.quitApp);
    expect(quitRow.action!.switchDeviceId, isNull);
  });
}
