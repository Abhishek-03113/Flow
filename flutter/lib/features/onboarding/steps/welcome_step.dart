import 'package:flutter/material.dart';

import '../../../core/theme/tokens.dart';
import '../../../core/widgets/app_logo.dart';
import '../../../core/widgets/primitives.dart';

/// Step 0 — first thing the user sees.
class WelcomeStep extends StatelessWidget {
  const WelcomeStep({
    super.key,
    required this.palette,
    required this.onContinue,
  });

  final FlowPalette palette;
  final VoidCallback onContinue;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        AppLogo(accent: c.accent),
        const SizedBox(height: 14),
        Text(
          'One keyboard. Every computer.',
          style: FlowType.heroTitle(c.text1).copyWith(fontSize: 20),
          textAlign: TextAlign.center,
        ),
        const SizedBox(height: 4),
        SizedBox(
          width: 350,
          child: Text(
            'Keep using the keyboard and mouse you already have. Press one key to move to the computer beside it.',
            style: FlowType.body(c.text2).copyWith(fontSize: 13, height: 1.5),
            textAlign: TextAlign.center,
          ),
        ),
        const SizedBox(height: 4),
        FlowButton(
          label: 'Continue',
          kind: FlowButtonKind.primary,
          background: c.accent,
          foreground: c.accentText,
          large: true,
          onPressed: onContinue,
        ),
      ],
    );
  }
}
