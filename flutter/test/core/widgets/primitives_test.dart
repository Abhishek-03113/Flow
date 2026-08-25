import 'package:flow_ui/core/platform_chrome.dart';
import 'package:flow_ui/core/widgets/primitives.dart';
import 'package:flow_ui/core/widgets/window_chrome.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('FlowToggle calls onChanged with the flipped value on tap', (
    tester,
  ) async {
    var value = false;
    await tester.pumpWidget(
      MaterialApp(
        home: StatefulBuilder(
          builder: (context, setState) => FlowToggle(
            value: value,
            activeColor: Colors.blue,
            trackColor: Colors.grey,
            onChanged: (v) => setState(() => value = v),
          ),
        ),
      ),
    );

    expect(value, isFalse);
    await tester.tap(find.byType(FlowToggle));
    await tester.pump();
    expect(value, isTrue);
  });

  testWidgets('FlowSegmentedControl reports the tapped segment', (
    tester,
  ) async {
    String? selected;
    await tester.pumpWidget(
      MaterialApp(
        home: FlowSegmentedControl<String>(
          segments: const [
            FlowSegment(value: 'a', label: 'A'),
            FlowSegment(value: 'b', label: 'B'),
          ],
          selected: 'a',
          onChanged: (v) => selected = v,
          background: Colors.black12,
          selectedBackground: Colors.blue,
          selectedForeground: Colors.white,
          unselectedForeground: Colors.black,
        ),
      ),
    );

    await tester.tap(find.text('B'));
    expect(selected, 'b');
  });

  testWidgets('FlowButton invokes onPressed', (tester) async {
    var tapped = false;
    await tester.pumpWidget(
      MaterialApp(
        home: FlowButton(
          label: 'Continue',
          kind: FlowButtonKind.primary,
          background: Colors.blue,
          foreground: Colors.white,
          onPressed: () => tapped = true,
        ),
      ),
    );

    await tester.tap(find.text('Continue'));
    expect(tapped, isTrue);
  });

  testWidgets('WindowChrome shows the right controls per platform', (
    tester,
  ) async {
    Future<void> pump(ChromeControls controls) => tester.pumpWidget(
      MaterialApp(
        home: WindowChrome(
          controls: controls,
          title: 'Cross Device',
          background: Colors.white,
          border: Colors.black12,
          textColor: Colors.black,
          textSecondary: Colors.black54,
        ),
      ),
    );

    await pump(ChromeControls.mac);
    expect(find.text('✕'), findsNothing);

    await pump(ChromeControls.win);
    expect(find.text('✕'), findsOneWidget);
    expect(find.text('□'), findsOneWidget);

    await pump(ChromeControls.gnome);
    expect(find.text('✕'), findsOneWidget);
  });
}
