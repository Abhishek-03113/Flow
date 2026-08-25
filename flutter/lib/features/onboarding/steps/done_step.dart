import 'package:flutter/material.dart';

import '../../../core/theme/tokens.dart';
import '../../../core/widgets/primitives.dart';

/// Step 3 — success. `trayName` and `switchKeyLabel` are daemon/platform
/// specific, not hardcoded.
class DoneStep extends StatelessWidget {
  const DoneStep({
    super.key,
    required this.palette,
    required this.trayName,
    required this.switchKeyLabel,
    required this.onDone,
  });

  final FlowPalette palette;
  final String trayName;
  final String switchKeyLabel;
  final VoidCallback onDone;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Container(
          width: 46,
          height: 46,
          decoration: const BoxDecoration(
            color: Color(0xFF30D158),
            shape: BoxShape.circle,
          ),
          alignment: Alignment.center,
          child: const Text(
            '✓',
            style: TextStyle(color: Colors.white, fontSize: 21),
          ),
        ),
        const SizedBox(height: 14),
        Text(
          'Ready',
          style: FlowType.heroTitle(c.text1).copyWith(fontSize: 20),
        ),
        const SizedBox(height: 4),
        SizedBox(
          width: 350,
          child: Text(
            'Cross Device lives in your $trayName from now on. Press $switchKeyLabel to switch computers.',
            style: FlowType.body(c.text2).copyWith(fontSize: 13, height: 1.5),
            textAlign: TextAlign.center,
          ),
        ),
        const SizedBox(height: 4),
        FlowButton(
          label: 'Done',
          kind: FlowButtonKind.primary,
          background: c.accent,
          foreground: c.accentText,
          large: true,
          onPressed: onDone,
        ),
      ],
    );
  }
}
