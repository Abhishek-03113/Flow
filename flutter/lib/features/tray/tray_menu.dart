import '../../domain/daemon_link_state.dart';
import '../../domain/device.dart';

/// What a tray menu row does when clicked. `null` action ⇒ a disabled
/// informational row.
sealed class TrayAction {
  const TrayAction();
  const factory TrayAction.switchDevice(String deviceId) = _SwitchDevice;
  static const pairNewDevice = _PairNewDevice();
  static const openDashboard = _OpenDashboard();
  static const openSettings = _OpenSettings();
  static const quitApp = _QuitApp();
}

class _SwitchDevice extends TrayAction {
  const _SwitchDevice(this.deviceId);
  final String deviceId;
  @override
  bool operator ==(Object other) =>
      other is _SwitchDevice && other.deviceId == deviceId;
  @override
  int get hashCode => deviceId.hashCode;
}

class _PairNewDevice extends TrayAction {
  const _PairNewDevice();
}

class _OpenDashboard extends TrayAction {
  const _OpenDashboard();
}

class _OpenSettings extends TrayAction {
  const _OpenSettings();
}

class _QuitApp extends TrayAction {
  const _QuitApp();
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

  entries.addAll(const [
    TrayMenuEntry.separator(),
    TrayMenuEntry(label: 'Pair New Device…', action: TrayAction.pairNewDevice),
    TrayMenuEntry.separator(),
    TrayMenuEntry(label: 'Dashboard', action: TrayAction.openDashboard),
    TrayMenuEntry(label: 'Settings', action: TrayAction.openSettings),
    TrayMenuEntry(label: 'Quit Flow', action: TrayAction.quitApp),
  ]);

  return entries;
}

extension<T> on Iterable<T> {
  T? get firstOrNull => isEmpty ? null : first;
}
