import 'switch_key_binding.dart';

enum PointerSensitivity { low, normal, high }

/// The daemon's persisted settings (`docs/contracts/data-model.md`).
class FlowSettings {
  const FlowSettings({
    required this.launchAtLogin,
    required this.showTrayIcon,
    required this.autoReconnect,
    required this.autoConnectPairedDevices,
    required this.shareKeyboard,
    required this.shareMouse,
    required this.debugLogging,
    required this.pointerSensitivity,
    required this.switchKey,
  });

  final bool launchAtLogin;
  final bool showTrayIcon;
  final bool autoReconnect;
  final bool autoConnectPairedDevices;
  final bool shareKeyboard;
  final bool shareMouse;
  final bool debugLogging;
  final PointerSensitivity pointerSensitivity;
  final SwitchKeyBinding switchKey;

  static FlowSettings defaults() {
    return FlowSettings(
      launchAtLogin: true,
      showTrayIcon: true,
      autoReconnect: true,
      autoConnectPairedDevices: true,
      shareKeyboard: true,
      shareMouse: true,
      debugLogging: false,
      pointerSensitivity: PointerSensitivity.normal,
      switchKey: SwitchKeyBinding.defaultBinding,
    );
  }

  FlowSettings applyPatch(SettingsPatch patch) {
    return FlowSettings(
      launchAtLogin: patch.launchAtLogin ?? launchAtLogin,
      showTrayIcon: patch.showTrayIcon ?? showTrayIcon,
      autoReconnect: patch.autoReconnect ?? autoReconnect,
      autoConnectPairedDevices:
          patch.autoConnectPairedDevices ?? autoConnectPairedDevices,
      shareKeyboard: patch.shareKeyboard ?? shareKeyboard,
      shareMouse: patch.shareMouse ?? shareMouse,
      debugLogging: patch.debugLogging ?? debugLogging,
      pointerSensitivity: patch.pointerSensitivity ?? pointerSensitivity,
      switchKey: patch.switchKey ?? switchKey,
    );
  }
}

/// A partial update to [FlowSettings] — only the fields being changed are
/// set. Sent as-is over the wire as `update_settings`'s payload
/// (`docs/contracts/daemon-ipc.md`); `set_switch_key` is a separate
/// command with its own validation, not folded into this patch.
class SettingsPatch {
  const SettingsPatch({
    this.launchAtLogin,
    this.showTrayIcon,
    this.autoReconnect,
    this.autoConnectPairedDevices,
    this.shareKeyboard,
    this.shareMouse,
    this.debugLogging,
    this.pointerSensitivity,
    this.switchKey,
  });

  final bool? launchAtLogin;
  final bool? showTrayIcon;
  final bool? autoReconnect;
  final bool? autoConnectPairedDevices;
  final bool? shareKeyboard;
  final bool? shareMouse;
  final bool? debugLogging;
  final PointerSensitivity? pointerSensitivity;
  final SwitchKeyBinding? switchKey;
}
