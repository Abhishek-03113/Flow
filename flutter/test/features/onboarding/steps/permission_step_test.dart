import 'package:flow_ui/core/theme/flow_theme.dart';
import 'package:flow_ui/domain/permission_status.dart';
import 'package:flow_ui/features/onboarding/steps/permission_step.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  const permission = PermissionStatus(
    name: 'Accessibility access',
    granted: false,
  );

  Widget host({required bool allowSkip, required VoidCallback onContinue}) =>
      MaterialApp(
        theme: FlowTheme.dark(),
        home: Builder(
          builder: (context) => Scaffold(
            body: Center(
              child: PermissionStep(
                palette: FlowColors.of(context),
                permission: permission,
                allowSkip: allowSkip,
                onGrant: () {},
                onContinue: onContinue,
              ),
            ),
          ),
        ),
      );

  testWidgets(
    'Continue stays disabled when ungranted and allowSkip is false (production default)',
    (tester) async {
      var continued = false;
      await tester.pumpWidget(
        host(allowSkip: false, onContinue: () => continued = true),
      );

      await tester.tap(find.text('Continue'));
      await tester.pump();

      expect(continued, isFalse);
    },
  );

  testWidgets(
    'Continue proceeds when ungranted but allowSkip is true (FLOW_ENV=development)',
    (tester) async {
      var continued = false;
      await tester.pumpWidget(
        host(allowSkip: true, onContinue: () => continued = true),
      );

      expect(
        find.textContaining("Development build"),
        findsOneWidget,
        reason: 'should explain why Continue is not blocking here',
      );

      await tester.tap(find.text('Continue'));
      await tester.pump();

      expect(continued, isTrue);
    },
  );
}
