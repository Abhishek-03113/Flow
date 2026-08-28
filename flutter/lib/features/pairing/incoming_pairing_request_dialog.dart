import 'package:flutter/material.dart';

import '../../domain/device.dart';
import '../../domain/pairing.dart';

/// Modal shown when another device asks to pair with this one. The only
/// consent gate — there is no auto-accept. Returns the user's choice, or
/// `null` if the dialog was dismissed (caller treats that as reject).
Future<PairingDecision?> showIncomingPairingRequestDialog(
  BuildContext context,
  IncomingPairingRequest request,
) {
  return showDialog<PairingDecision>(
    context: context,
    barrierDismissible: true,
    builder: (_) => IncomingPairingRequestDialog(request: request),
  );
}

class IncomingPairingRequestDialog extends StatelessWidget {
  const IncomingPairingRequestDialog({super.key, required this.request});

  final IncomingPairingRequest request;

  String get _osLabel => switch (request.deviceOs) {
    HostOs.macos => 'macOS',
    HostOs.windows => 'Windows',
    HostOs.linux => 'Linux',
  };

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Pair this device?'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            request.deviceName,
            style: Theme.of(context).textTheme.titleMedium,
          ),
          const SizedBox(height: 4),
          Text(
            request.address.isEmpty
                ? _osLabel
                : '$_osLabel · ${request.address}',
            style: Theme.of(context).textTheme.bodySmall,
          ),
          const SizedBox(height: 16),
          const Text('Verification code'),
          const SizedBox(height: 4),
          SelectableText(
            request.fingerprint,
            style: const TextStyle(
              fontFamily: 'monospace',
              fontSize: 16,
              letterSpacing: 1.5,
            ),
          ),
          const SizedBox(height: 4),
          Text(
            'Confirm this matches the code shown on the other device.',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(PairingDecision.reject),
          child: const Text('Reject'),
        ),
        FilledButton(
          onPressed: () => Navigator.of(context).pop(PairingDecision.accept),
          child: const Text('Accept'),
        ),
      ],
    );
  }
}
