import 'package:flow_ui/core/theme/flow_theme.dart';
import 'package:flow_ui/core/widgets/toast.dart';
import 'package:flow_ui/state/ui_providers.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('ToastOverlay shows a message and auto-dismisses it', (
    tester,
  ) async {
    final container = ProviderContainer();
    addTearDown(container.dispose);

    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: MaterialApp(
          theme: FlowTheme.dark(),
          home: const Stack(children: [ToastOverlay()]),
        ),
      ),
    );

    expect(find.text('Switched'), findsNothing);

    container.read(toastProvider.notifier).show('Switched');
    await tester.pump();
    await tester.pump(
      const Duration(milliseconds: 250),
    ); // AnimatedSwitcher transition
    expect(find.text('Switched'), findsOneWidget);

    await tester.pump(const Duration(milliseconds: 1700));
    await tester.pump(const Duration(milliseconds: 250));
    expect(find.text('Switched'), findsNothing);
  });

  testWidgets('a new toast replaces the current one instead of queuing', (
    tester,
  ) async {
    final container = ProviderContainer();
    addTearDown(container.dispose);

    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: MaterialApp(
          theme: FlowTheme.dark(),
          home: const Stack(children: [ToastOverlay()]),
        ),
      ),
    );

    container.read(toastProvider.notifier).show('First');
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 250));
    expect(find.text('First'), findsOneWidget);

    container.read(toastProvider.notifier).show('Second');
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 250));
    expect(find.text('First'), findsNothing);
    expect(find.text('Second'), findsOneWidget);

    // Let "Second"'s own auto-dismiss timer run to completion — a timer
    // still pending when the test ends trips flutter_test's invariant
    // check, since that check runs before addTearDown callbacks fire.
    await tester.pump(const Duration(milliseconds: 1700));
    await tester.pump(const Duration(milliseconds: 250));
  });
}
