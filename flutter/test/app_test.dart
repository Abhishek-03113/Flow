import 'package:flow_ui/app.dart';
import 'package:flow_ui/data/mock_daemon_repository.dart';
import 'package:flow_ui/state/repository_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  setUp(() {
    // A fresh install: no onboarding-complete flag saved yet.
    SharedPreferences.setMockInitialValues({});
  });

  testWidgets('FlowApp launches into onboarding on a fresh install', (
    tester,
  ) async {
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          daemonRepositoryProvider.overrideWithValue(MockDaemonRepository()),
        ],
        child: const FlowApp(),
      ),
    );
    await tester.pump();

    expect(find.text('One keyboard. Every computer.'), findsOneWidget);
  });

  testWidgets('FlowApp launches into the dashboard once onboarded', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues({'flow.onboarding_complete': true});

    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          daemonRepositoryProvider.overrideWithValue(MockDaemonRepository()),
        ],
        child: const FlowApp(),
      ),
    );
    await tester.pump();
    await tester.pump();

    // Assert the dashboard actually rendered, not merely that onboarding
    // is absent: the loading state is also free of the onboarding
    // headline, so checking only for its absence would pass on an empty
    // window. "Overview" is the app window's own sidebar heading.
    expect(find.text('Overview'), findsOneWidget);
    expect(find.text('One keyboard. Every computer.'), findsNothing);
  });
}
