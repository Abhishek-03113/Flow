import '../../domain/daemon_link_state.dart';
import '../../domain/device.dart';

enum TrayActionKind {
  switchDevice,
  pairNewDevice,
  openDashboard,
  openSettings,
  quitApp,
}

/// What a tray menu row does when clicked. `null` action ⇒ a disabled
/// informational row.
class TrayAction {
  const TrayAction._(this.kind, [this.switchDeviceId]);

  /// Named const factory constructors for each action type.
  const factory TrayAction.switchDevice(String deviceId) = _SwitchDeviceImpl;
  const factory TrayAction.pairNewDevice() = _PairNewDeviceImpl;
  const factory TrayAction.openDashboard() = _OpenDashboardImpl;
  const factory TrayAction.openSettings() = _OpenSettingsImpl;
  const factory TrayAction.quitApp() = _QuitAppImpl;

  final TrayActionKind kind;
  final String? switchDeviceId;

  @override
  bool operator ==(Object other) =>
      other is TrayAction &&
      other.kind == kind &&
      other.switchDeviceId == switchDeviceId;

  @override
  int get hashCode => Object.hash(kind, switchDeviceId);
}

class _SwitchDeviceImpl extends TrayAction {
  const _SwitchDeviceImpl(String deviceId)
    : super._(TrayActionKind.switchDevice, deviceId);
}

class _PairNewDeviceImpl extends TrayAction {
  const _PairNewDeviceImpl() : super._(TrayActionKind.pairNewDevice);
}

class _OpenDashboardImpl extends TrayAction {
  const _OpenDashboardImpl() : super._(TrayActionKind.openDashboard);
}

class _OpenSettingsImpl extends TrayAction {
  const _OpenSettingsImpl() : super._(TrayActionKind.openSettings);
}

class _QuitAppImpl extends TrayAction {
  const _QuitAppImpl() : super._(TrayActionKind.quitApp);
}

class TrayMenuEntry {
  const TrayMenuEntry({
    required this.label,
    this.enabled = true,
    this.action,
    this.isSeparator = false,
  });

  const TrayMenuEntry.separator()
    : label = '',
      enabled = false,
      action = null,
      isSeparator = true;

  final String label;
  final bool enabled;
  final TrayAction? action;
  final bool isSeparator;
}

String _linkLabel(DaemonLinkState link) => switch (link) {
  DaemonLinkState.connected => 'Connected',
  DaemonLinkState.connecting => 'Connecting…',
  DaemonLinkState.reconnecting => 'Reconnecting…',
  DaemonLinkState.disconnected => 'Disconnected',
  DaemonLinkState.error => 'Connection lost',
  DaemonLinkState.permissionRequired => 'Needs permission',
};

String _osLabel(HostOs os) => switch (os) {
  HostOs.macos => 'macOS',
  HostOs.windows => 'Windows',
  HostOs.linux => 'Linux',
};

/// The tray/menu-bar menu, derived from live daemon state. Mirrors the
/// information architecture of `design/claude-design-export/TrayPopover.dc.html`
/// as far as a native menu allows (no inline pairing progress).
List<TrayMenuEntry> buildTrayMenu({
  required DaemonLinkState link,
  required List<Device> devices,
  required String localDeviceId,
}) {
  final entries = <TrayMenuEntry>[
    TrayMenuEntry(label: 'Flow — ${_linkLabel(link)}', enabled: false),
    const TrayMenuEntry.separator(),
  ];

  final active = devices
      .where((d) => d.id != localDeviceId && d.state == DeviceState.active)
      .firstOrNull;
  // "This device" is the local record; show it as the active row when
  // nothing remote is active (matches the popover's "Using" card).
  final local = devices.where((d) => d.id == localDeviceId).firstOrNull;
  final usingDevice = active ?? local;
  if (usingDevice != null) {
    entries.add(
      TrayMenuEntry(
        label: 'Using: ${usingDevice.name} (${_osLabel(usingDevice.os)})',
        enabled: false,
      ),
    );
  }

  for (final d in devices.where((d) => d.id != localDeviceId)) {
    if (d.state == DeviceState.inactive || d.state == DeviceState.connected) {
      entries.add(
        TrayMenuEntry(
          label: 'Switch to ${d.name}',
          action: TrayAction.switchDevice(d.id),
        ),
      );
    } else if (d.state == DeviceState.disconnected ||
        d.state == DeviceState.error) {
      entries.add(
        TrayMenuEntry(label: '${d.name} — disconnected', enabled: false),
      );
    }
  }

  entries.addAll([
    const TrayMenuEntry.separator(),
    const TrayMenuEntry(
      label: 'Pair New Device…',
      action: TrayAction.pairNewDevice(),
    ),
    const TrayMenuEntry.separator(),
    const TrayMenuEntry(label: 'Dashboard', action: TrayAction.openDashboard()),
    const TrayMenuEntry(label: 'Settings', action: TrayAction.openSettings()),
    const TrayMenuEntry(label: 'Quit Flow', action: TrayAction.quitApp()),
  ]);

  return entries;
}

extension<T> on Iterable<T> {
  T? get firstOrNull => isEmpty ? null : first;
}
