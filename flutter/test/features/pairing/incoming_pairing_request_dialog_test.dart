import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/domain/pairing.dart';
import 'package:flow_ui/features/pairing/incoming_pairing_request_dialog.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final request = const IncomingPairingRequest(
    requestId: 'ipr-1',
    deviceName: 'Windows Box',
    deviceOs: HostOs.windows,
    fingerprint: '3f2a 91c4 8d10 6b57',
    address: '192.168.0.103',
  );

  testWidgets('renders identity and returns Accept', (tester) async {
    late Future<PairingDecision?> result;
    await tester.pumpWidget(
      MaterialApp(
        home: Builder(
          builder: (context) => ElevatedButton(
            onPressed: () =>
                result = showIncomingPairingRequestDialog(context, request),
            child: const Text('go'),
          ),
        ),
      ),
    );
    await tester.tap(find.text('go'));
    await tester.pumpAndSettle();

    expect(find.text('Windows Box'), findsOneWidget);
    expect(find.textContaining('3f2a 91c4 8d10 6b57'), findsOneWidget);
    expect(find.textContaining('192.168.0.103'), findsOneWidget);

    await tester.tap(find.text('Accept'));
    await tester.pumpAndSettle();
    expect(await result, PairingDecision.accept);
  });

  testWidgets('Reject returns reject', (tester) async {
    late Future<PairingDecision?> result;
    await tester.pumpWidget(
      MaterialApp(
        home: Builder(
          builder: (context) => ElevatedButton(
            onPressed: () =>
                result = showIncomingPairingRequestDialog(context, request),
            child: const Text('go'),
          ),
        ),
      ),
    );
    await tester.tap(find.text('go'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Reject'));
    await tester.pumpAndSettle();
    expect(await result, PairingDecision.reject);
  });
}
