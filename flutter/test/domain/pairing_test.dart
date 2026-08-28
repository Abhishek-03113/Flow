import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/domain/pairing.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('incomingPairingRequestFromJson parses a full payload', () {
    final req = incomingPairingRequestFromJson({
      'request_id': 'ipr-1',
      'device_name': 'Windows Box',
      'device_os': 'windows',
      'fingerprint': '3f2a 91c4 8d10 6b57',
      'address': '192.168.0.103',
    });
    expect(req, isNotNull);
    expect(req!.requestId, 'ipr-1');
    expect(req.deviceOs, HostOs.windows);
    expect(req.fingerprint, '3f2a 91c4 8d10 6b57');
  });

  test('incomingPairingRequestFromJson returns null for null', () {
    expect(incomingPairingRequestFromJson(null), isNull);
  });

  test('PairingDecision.wireName', () {
    expect(PairingDecision.accept.wireName, 'accept');
    expect(PairingDecision.reject.wireName, 'reject');
  });
}
