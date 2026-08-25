import '../domain/device.dart';
import 'theme/tokens.dart';

/// Which edge of the screen a platform's menu bar / taskbar sits on.
enum BarPosition { top, bottom }

/// The window-decoration style a platform uses — which controls
/// [WindowChrome] (`core/widgets/window_chrome.dart`) renders.
enum ChromeControls { mac, win, gnome }

/// Per-platform metadata for rendering the desktop bar and window chrome,
/// mirroring `platformMeta()` in the Claude Design source
/// (`todos.json`'s `sharedDesignTokens.platformChrome`).
///
/// Keyed by [HostOs] (`domain/device.dart`) rather than a second
/// "HostPlatform" enum — a daemon-reported device OS and "which platform
/// is this chrome for" are the same three values, and a parallel enum
/// would only invite the two to drift.
class PlatformChrome {
  const PlatformChrome({
    required this.os,
    required this.barPosition,
    required this.barHeight,
    required this.controls,
    required this.trayName,
    required this.permissionName,
  });

  final HostOs os;
  final BarPosition barPosition;
  final double barHeight;
  final ChromeControls controls;

  /// "menu bar" / "system tray" / "tray" — used in copy like "Cross
  /// Device lives in your {trayName} from now on."
  final String trayName;

  /// "Accessibility access" / "Input access" / "Input device access" —
  /// daemon-supplied in the real contract (`PermissionStatus.name`); this
  /// static copy is only a fallback for previewing platforms the mock
  /// isn't currently impersonating (see the dev harness, `todos.json` S2).
  final String permissionName;

  static const windowChromeBarHeight = 40.0;

  static const _macos = PlatformChrome(
    os: HostOs.macos,
    barPosition: BarPosition.top,
    barHeight: 26,
    controls: ChromeControls.mac,
    trayName: 'menu bar',
    permissionName: 'Accessibility access',
  );

  static const _windows = PlatformChrome(
    os: HostOs.windows,
    barPosition: BarPosition.bottom,
    barHeight: 44,
    controls: ChromeControls.win,
    trayName: 'system tray',
    permissionName: 'Input access',
  );

  static const _linux = PlatformChrome(
    os: HostOs.linux,
    barPosition: BarPosition.top,
    barHeight: 30,
    controls: ChromeControls.gnome,
    trayName: 'tray',
    permissionName: 'Input device access',
  );

  static PlatformChrome of(HostOs os) {
    return switch (os) {
      HostOs.macos => _macos,
      HostOs.windows => _windows,
      HostOs.linux => _linux,
    };
  }

  double windowRadius() {
    return switch (os) {
      HostOs.macos => FlowRadii.macWindow,
      HostOs.windows => FlowRadii.windowsWindow,
      HostOs.linux => FlowRadii.linuxWindow,
    };
  }
}
