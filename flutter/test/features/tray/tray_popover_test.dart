import 'package:flow_ui/core/theme/flow_theme.dart';
import 'package:flow_ui/core/widgets/toast.dart';
import 'package:flow_ui/data/mock_daemon_repository.dart';
import 'package:flow_ui/domain/daemon_link_state.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/features/tray/tray_popover.dart';
import 'package:flow_ui/state/repository_providers.dart';
import 'package:flutter/material.dart';
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

  Widget host() => UncontrolledProviderScope(
    container: container,
    child: MaterialApp(
      theme: FlowTheme.dark(),
      home: Scaffold(
        body: Stack(
          children: [
            Center(child: TrayPopover(platform: HostOs.macos)),
            const ToastOverlay(),
          ],
        ),
      ),
    ),
  );

  testWidgets(
    'shows the active device in the Using card and other devices below',
    (tester) async {
      await tester.pumpWidget(host());
      await tester.pump();

      expect(find.text('MacBook'), findsOneWidget);
      expect(find.textContaining('Active'), findsOneWidget);
      expect(find.text('Work Laptop'), findsOneWidget);
      expect(find.text('Desktop'), findsOneWidget);
    },
  );

  testWidgets('tapping a switchable device switches it and shows a toast', (
    tester,
  ) async {
    await tester.pumpWidget(host());
    await tester.pump();

    await tester.tap(find.text('Work Laptop'));
    await tester.pump(); // starts the 400ms debounce
    expect(find.text('Switching…'), findsOneWidget);

    await tester.pump(const Duration(milliseconds: 450));
    await tester.pump(const Duration(milliseconds: 250)); // toast fade-in
    expect(find.text('Switched'), findsOneWidget);

    // Read the provider's cached value rather than repo.watchDevices().first
    // here: subscribing directly to the repository's Stream.multi channel
    // from outside the widget tree, then letting that subscription's
    // cancellation race the FakeAsync test zone, was observed to wedge
    // every subsequent tester.pump(duration) call indefinitely.
    final devices = container.read(devicesProvider).valueOrNull ?? const [];
    expect(devices.singleWhere((d) => d.id == 'd2').state, DeviceState.active);

    await tester.pump(const Duration(milliseconds: 1700));
    await tester.pump(const Duration(milliseconds: 250));
  });

  testWidgets('disconnected device is not tappable and shows no arrow', (
    tester,
  ) async {
    await tester.pumpWidget(host());
    await tester.pump();

    final desktopRow = find
        .ancestor(
          of: find.text('Desktop'),
          matching: find.byType(GestureDetector),
        )
        .first;
    final gesture = tester.widget<GestureDetector>(desktopRow);
    expect(gesture.onTap, isNull);
  });

  for (final entry in {
    DaemonLinkState.connected: null,
    DaemonLinkState.connecting: null,
    DaemonLinkState.reconnecting: 'Work Laptop dropped out. Trying again.',
    DaemonLinkState.disconnected: 'Work Laptop is unavailable.',
    DaemonLinkState.error: 'Input sharing paused until Work Laptop is back.',
    DaemonLinkState.permissionRequired:
        'Allow input access to share your keyboard.',
  }.entries) {
    testWidgets('banner for ${entry.key} matches the design copy', (
      tester,
    ) async {
      repo.debugSetLinkState(entry.key);
      await tester.pumpWidget(host());
      await tester.pump();

      if (entry.value == null) {
        expect(find.textContaining('dropped out'), findsNothing);
        expect(find.textContaining('unavailable'), findsNothing);
        expect(find.textContaining('paused'), findsNothing);
        expect(find.textContaining('Allow input access'), findsNothing);
      } else {
        expect(find.text(entry.value!), findsOneWidget);
      }
    });
  }

  testWidgets('pairing flow renders all four stages', (tester) async {
    await tester.pumpWidget(host());
    await tester.pump();

    await tester.tap(find.text('Pair New Device'));
    await tester.pump();
    expect(find.text('Searching for devices…'), findsOneWidget);

    await tester.pump(const Duration(milliseconds: 1250));
    expect(find.text('Office Mac Mini'), findsOneWidget);

    await tester.tap(find.text('Pair').first);
    await tester.pump();
    expect(find.text('Waiting for approval…'), findsOneWidget);

    await tester.pump(const Duration(milliseconds: 1550));
    expect(find.text('Office Mac Mini connected'), findsOneWidget);

    await tester.pump(const Duration(milliseconds: 1650));
    expect(find.text('Cross Device'), findsOneWidget); // back to the main menu
  }, timeout: const Timeout(Duration(seconds: 10)));
}
