import '../domain/daemon_link_state.dart';
import '../domain/device.dart';
import '../domain/pairing.dart';
import '../domain/permission_status.dart';
import '../domain/settings.dart';
import '../domain/switch_key_binding.dart';

/// JSON <-> domain-type conversions for the IPC wire format
/// (`docs/contracts/data-model.md`). Kept out of `lib/domain/` so the
/// domain layer stays wire-format-agnostic — only [IpcDaemonRepository]
/// needs to know these shapes exist.

Device deviceFromJson(Map<String, dynamic> json) {
  return Device(
    id: json['id'] as String,
    name: json['name'] as String,
    os: HostOs.values.byName(json['os'] as String),
    state: DeviceState.values.byName(json['state'] as String),
    lastSeen: DateTime.parse(json['last_seen'] as String),
  );
}

List<Device> devicesFromJson(dynamic json) {
  return (json as List<dynamic>)
      .map((e) => deviceFromJson(e as Map<String, dynamic>))
      .toList();
}

/// `permissionRequired` is the one variant whose Dart camelCase name
/// doesn't match its snake_case wire form (`permission_required`) via a
/// plain [Enum.byName] lookup — everything else here is a single word.
DaemonLinkState daemonLinkStateFromJson(String json) {
  if (json == 'permission_required') return DaemonLinkState.permissionRequired;
  return DaemonLinkState.values.byName(json);
}

PairingCandidate pairingCandidateFromJson(Map<String, dynamic> json) {
  return PairingCandidate(
    id: json['id'] as String,
    name: json['name'] as String,
    os: HostOs.values.byName(json['os'] as String),
  );
}

PairingSession pairingSessionFromJson(Map<String, dynamic> json) {
  return PairingSession(
    stage: PairingStage.values.byName(json['stage'] as String),
    candidates: (json['candidates'] as List<dynamic>)
        .map((e) => pairingCandidateFromJson(e as Map<String, dynamic>))
        .toList(),
    targetName: json['target_name'] as String?,
    error: json['error'] as String?,
  );
}

SwitchKeyBinding switchKeyBindingFromJson(Map<String, dynamic> json) {
  return SwitchKeyBinding(
    label: json['label'] as String,
    keys: (json['keys'] as List<dynamic>).cast<String>(),
  );
}

FlowSettings flowSettingsFromJson(Map<String, dynamic> json) {
  return FlowSettings(
    launchAtLogin: json['launch_at_login'] as bool,
    showTrayIcon: json['show_tray_icon'] as bool,
    autoReconnect: json['auto_reconnect'] as bool,
    autoConnectPairedDevices: json['auto_connect_paired_devices'] as bool,
    shareKeyboard: json['share_keyboard'] as bool,
    shareMouse: json['share_mouse'] as bool,
    debugLogging: json['debug_logging'] as bool,
    pointerSensitivity: PointerSensitivity.values.byName(
      json['pointer_sensitivity'] as String,
    ),
    switchKey: switchKeyBindingFromJson(
      json['switch_key'] as Map<String, dynamic>,
    ),
  );
}

PermissionStatus permissionStatusFromJson(Map<String, dynamic> json) {
  return PermissionStatus(
    name: json['name'] as String,
    granted: json['granted'] as bool,
  );
}
