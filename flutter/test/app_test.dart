import 'package:flow_ui/app.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('FlowApp launches to the dev harness', (tester) async {
    await tester.pumpWidget(const ProviderScope(child: FlowApp()));
    await tester.pump();

    expect(find.text('Cross Device (dev harness)'), findsOneWidget);
  });
}
