import 'package:flow_ui/core/theme/flow_theme.dart';
import 'package:flow_ui/data/mock_daemon_repository.dart';
import 'package:flow_ui/features/harness/dev_harness.dart';
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
    child: MaterialApp(theme: FlowTheme.dark(), home: const DevHarness()),
  );

  testWidgets('defaults to the Menu Bar view with the tray popover open', (
    tester,
  ) async {
    await tester.pumpWidget(host());
    await tester.pump();

    expect(find.text('Cross Device (dev harness)'), findsOneWidget);
    expect(
      find.text('MacBook'),
      findsOneWidget,
    ); // active device in the popover
  });

  testWidgets('Connection segment forces the link state on the mock', (
    tester,
  ) async {
    await tester.pumpWidget(host());
    await tester.pump();

    expect(
      find.text('Connected'),
      findsWidgets,
    ); // segment label + popover status

    // The control strip scrolls horizontally; scroll "Disconnected" into
    // view before tapping it.
    await tester.ensureVisible(find.text('Disconnected').first);
    await tester.pump();
    await tester.tap(find.text('Disconnected').first);
    await tester.pump();

    expect(find.text('the other device is unavailable.'), findsOneWidget);
  });

  testWidgets(
    'All Platforms view renders all three platform frames with popovers',
    (tester) async {
      await tester.pumpWidget(host());
      await tester.pump();

      await tester.tap(find.text('All Platforms'));
      await tester.pump();

      // "macOS"/"Windows"/"Linux" each match twice: once in the Platform
      // segmented control, once as a frame title.
      expect(find.text('macOS'), findsNWidgets(2));
      expect(find.text('Windows'), findsNWidgets(2));
      expect(find.text('Linux'), findsNWidgets(2));
      expect(
        find.text('MacBook'),
        findsNWidgets(3),
      ); // active device shown in all 3 popovers
    },
  );

  testWidgets('App Window and First Launch views render their surfaces', (
    tester,
  ) async {
    await tester.pumpWidget(host());
    await tester.pump();

    await tester.tap(find.text('App Window'));
    await tester.pump();
    expect(find.text('Controlling'), findsOneWidget);

    await tester.tap(find.text('First Launch'));
    await tester.pump();
    expect(find.text('One keyboard. Every computer.'), findsOneWidget);
  });
}
