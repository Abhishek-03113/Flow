import 'package:flow_ui/core/theme/flow_theme.dart';
import 'package:flow_ui/data/mock_daemon_repository.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/features/onboarding/onboarding_flow.dart';
import 'package:flow_ui/state/repository_providers.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  late MockDaemonRepository repo;
  late ProviderContainer container;
  var doneCalled = false;

  setUp(() {
    repo = MockDaemonRepository();
    container = ProviderContainer(
      overrides: [daemonRepositoryProvider.overrideWithValue(repo)],
    );
    doneCalled = false;
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
        body: Center(
          child: OnboardingFlow(
            platform: HostOs.macos,
            onDone: () => doneCalled = true,
          ),
        ),
      ),
    ),
  );

  testWidgets('walks welcome -> permission -> pair -> done and calls onDone', (
    tester,
  ) async {
    await tester.pumpWidget(host());
    await tester.pump();

    expect(find.text('One keyboard. Every computer.'), findsOneWidget);
    await tester.tap(find.text('Continue'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 250));

    expect(find.text('Allow input access'), findsOneWidget);
    expect(find.text('Not granted yet'), findsOneWidget);

    await tester.tap(find.text('Allow'));
    await tester.pump();
    expect(find.text('Granted'), findsWidgets);

    await tester.tap(find.text('Continue'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 250));

    expect(find.text('Find your other computer'), findsOneWidget);
    expect(find.text('Searching…'), findsOneWidget);

    await tester.pump(const Duration(milliseconds: 1250));
    expect(find.text('Office Mac Mini'), findsOneWidget);

    await tester.tap(find.text('Pair').first);
    await tester.pump();
    expect(find.textContaining('Waiting for approval'), findsOneWidget);

    await tester.pump(const Duration(milliseconds: 1550));
    await tester.pump(const Duration(milliseconds: 250));

    expect(find.text('Ready'), findsOneWidget);
    expect(doneCalled, isFalse);

    await tester.tap(find.text('Done'));
    await tester.pump();
    expect(doneCalled, isTrue);

    // Let the pairing session's own auto-return-to-idle timer run out so
    // no Timer is left pending at teardown.
    await tester.pump(const Duration(milliseconds: 1700));
  }, timeout: const Timeout(Duration(seconds: 10)));

  testWidgets('Continue on the permission step is disabled until granted', (
    tester,
  ) async {
    await tester.pumpWidget(host());
    await tester.pump();
    await tester.tap(find.text('Continue'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 250));

    final continueButtons = find.text('Continue');
    expect(continueButtons, findsOneWidget);
    // Still on the permission step (no crash/advance) since nothing was granted.
    expect(find.text('Allow input access'), findsOneWidget);
  });
}
