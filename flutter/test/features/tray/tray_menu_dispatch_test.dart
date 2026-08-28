import 'package:flow_ui/app.dart'
    show
        QuitEffect,
        ShowWindowEffect,
        StartPairingThenShowEffect,
        SwitchDeviceEffect,
        TrayActionEffect,
        resolveTrayAction;
import 'package:flow_ui/features/app_window/app_window_shell.dart';
import 'package:flow_ui/features/tray/tray_menu.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('switchDevice → SwitchDeviceEffect carrying the device id', () {
    final TrayActionEffect e = resolveTrayAction(
      const TrayAction.switchDevice('d2'),
    );
    expect(e, isA<SwitchDeviceEffect>());
    expect((e as SwitchDeviceEffect).deviceId, 'd2');
  });

  test('openSettings → ShowWindowEffect on the General section', () {
    final e = resolveTrayAction(const TrayAction.openSettings());
    expect(e, isA<ShowWindowEffect>());
    expect((e as ShowWindowEffect).section, AppSection.general);
  });

  test('pairNewDevice → StartPairingThenShowEffect on the Dashboard', () {
    final e = resolveTrayAction(const TrayAction.pairNewDevice());
    expect(e, isA<StartPairingThenShowEffect>());
    expect((e as StartPairingThenShowEffect).section, AppSection.dashboard);
  });

  test('openDashboard → ShowWindowEffect on the Dashboard section', () {
    final e = resolveTrayAction(const TrayAction.openDashboard());
    expect(e, isA<ShowWindowEffect>());
    expect((e as ShowWindowEffect).section, AppSection.dashboard);
  });

  test('quitApp → QuitEffect', () {
    expect(resolveTrayAction(const TrayAction.quitApp()), isA<QuitEffect>());
  });
}
