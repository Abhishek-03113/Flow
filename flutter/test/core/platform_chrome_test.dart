import 'package:flow_ui/core/platform_chrome.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('macOS chrome matches the design source', () {
    final chrome = PlatformChrome.of(HostOs.macos);
    expect(chrome.barPosition, BarPosition.top);
    expect(chrome.barHeight, 26);
    expect(chrome.controls, ChromeControls.mac);
    expect(chrome.trayName, 'menu bar');
    expect(chrome.permissionName, 'Accessibility access');
  });

  test('Windows chrome matches the design source', () {
    final chrome = PlatformChrome.of(HostOs.windows);
    expect(chrome.barPosition, BarPosition.bottom);
    expect(chrome.barHeight, 44);
    expect(chrome.controls, ChromeControls.win);
    expect(chrome.trayName, 'system tray');
    expect(chrome.permissionName, 'Input access');
  });

  test('Linux chrome matches the design source', () {
    final chrome = PlatformChrome.of(HostOs.linux);
    expect(chrome.barPosition, BarPosition.top);
    expect(chrome.barHeight, 30);
    expect(chrome.controls, ChromeControls.gnome);
    expect(chrome.trayName, 'tray');
    expect(chrome.permissionName, 'Input device access');
  });
}
