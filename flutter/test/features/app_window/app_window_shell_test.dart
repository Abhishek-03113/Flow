import 'package:flow_ui/core/theme/flow_theme.dart';
import 'package:flow_ui/core/widgets/primitives.dart';
import 'package:flow_ui/core/widgets/toast.dart';
import 'package:flow_ui/data/mock_daemon_repository.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/features/app_window/app_window_shell.dart';
import 'package:flow_ui/state/repository_providers.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  late MockDaemonRepository repo;
  late ProviderContainer container;

  setUp(() {
    repo = MockDaemonRepository();
    container = ProviderContainer(
      overrides: [daemonRepositoryProvider.overrideWithValue(repo)],
    );
  });

  tearDown(() {
    container.dispose();
    repo.dispose();
  });

  Widget host({AppSection initialSection = AppSection.dashboard}) =>
      UncontrolledProviderScope(
        container: container,
        child: MaterialApp(
          theme: FlowTheme.dark(),
          home: Scaffold(
            body: Stack(
              children: [
                Center(
                  child: AppWindowShell(
                    platform: HostOs.macos,
                    initialSection: initialSection,
                  ),
                ),
                const ToastOverlay(),
              ],
            ),
          ),
        ),
      );

  testWidgets('sidebar navigation switches sections and the window title', (
    tester,
  ) async {
    await tester.pumpWidget(host());
    await tester.pump();

    expect(
      find.text('Cross Device'),
      findsOneWidget,
    ); // WindowChrome title on dashboard
    expect(find.text('Controlling'), findsOneWidget);

    await tester.tap(find.text('General'));
    await tester.pump();
    // "Settings" now matches both the WindowChrome title and the
    // sidebar's "Settings" group label.
    expect(find.text('Settings'), findsNWidgets(2));
    expect(find.text('Launch at login'), findsOneWidget);
  });

  testWidgets('dashboard: switching a device via the Switch button works', (
    tester,
  ) async {
    await tester.pumpWidget(host());
    await tester.pump();

    await tester.tap(find.text('Switch'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 450));
    await tester.pump(const Duration(milliseconds: 250));

    expect(find.text('Switched'), findsOneWidget);
    expect(
      container
          .read(devicesProvider)
          .valueOrNull!
          .singleWhere((d) => d.id == 'd2')
          .state,
      DeviceState.active,
    );

    await tester.pump(const Duration(milliseconds: 1700));
    await tester.pump(const Duration(milliseconds: 250));
  });

  testWidgets('dashboard: removing a device works and shows a toast', (
    tester,
  ) async {
    await tester.pumpWidget(host());
    await tester.pump();

    final desktopRow = find
        .ancestor(of: find.text('Desktop'), matching: find.byType(Container))
        .first;
    final removeButton = find.descendant(
      of: desktopRow,
      matching: find.text('✕'),
    );
    await tester.tap(removeButton);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 250));

    expect(find.text('Device removed'), findsOneWidget);
    expect(
      container.read(devicesProvider).valueOrNull!.any((d) => d.id == 'd3'),
      isFalse,
    );

    await tester.pump(const Duration(milliseconds: 1700));
    await tester.pump(const Duration(milliseconds: 250));
  });

  testWidgets('general: toggling launch at login updates settings', (
    tester,
  ) async {
    await tester.pumpWidget(host(initialSection: AppSection.general));
    await tester.pump();

    expect(container.read(settingsProvider).valueOrNull!.launchAtLogin, isTrue);
    await tester.tap(find.byType(FlowToggle).first);
    await tester.pump();
    expect(
      container.read(settingsProvider).valueOrNull!.launchAtLogin,
      isFalse,
    );
  });

  testWidgets('devices: shows last-seen meta for each device', (tester) async {
    await tester.pumpWidget(host(initialSection: AppSection.devices));
    await tester.pump();

    expect(find.textContaining('last seen'), findsWidgets);
  });

  testWidgets('input: tapping a preset chip sets the switch key', (
    tester,
  ) async {
    await tester.pumpWidget(host(initialSection: AppSection.input));
    await tester.pump();

    expect(find.text('Scroll Lock'), findsWidgets);
    await tester.tap(find.text('F13'));
    await tester.pump();

    expect(
      container.read(settingsProvider).valueOrNull!.switchKey.label,
      'F13',
    );
  });

  testWidgets('input: recording captures a key combo', (tester) async {
    await tester.pumpWidget(host(initialSection: AppSection.input));
    await tester.pump();

    await tester.tap(find.text('Record shortcut'));
    await tester.pump();
    expect(find.text('Press any key…'), findsOneWidget);

    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.keyK);
    await tester.pump();
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyK);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);

    expect(
      container.read(settingsProvider).valueOrNull!.switchKey.label,
      'Ctrl + K',
    );

    // The "Switch key updated" toast this triggers has its own auto-
    // dismiss timer; let it finish before the test ends.
    await tester.pump(const Duration(milliseconds: 1700));
    await tester.pump(const Duration(milliseconds: 250));
  });

  testWidgets('advanced: reset button resets settings and shows a toast', (
    tester,
  ) async {
    await tester.pumpWidget(host(initialSection: AppSection.advanced));
    await tester.pump();

    await tester.tap(find.text('Reset all settings'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 250));

    expect(find.text('Settings reset'), findsOneWidget);

    await tester.pump(const Duration(milliseconds: 1700));
    await tester.pump(const Duration(milliseconds: 250));
  });
}
