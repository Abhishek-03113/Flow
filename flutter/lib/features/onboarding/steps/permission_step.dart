import 'package:flutter/material.dart';

import '../../../core/theme/tokens.dart';
import '../../../core/widgets/primitives.dart';
import '../../../domain/permission_status.dart';

/// Step 1 — grant the OS input-capture permission. "Continue" stays
/// muted/disabled until [permission] is granted.
class PermissionStep extends StatelessWidget {
  const PermissionStep({
    super.key,
    required this.palette,
    required this.permission,
    required this.onGrant,
    required this.onContinue,
  });

  final FlowPalette palette;
  final PermissionStatus permission;
  final VoidCallback onGrant;
  final VoidCallback onContinue;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    final granted = permission.granted;
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          'Allow input access',
          style: FlowType.heroTitle(c.text1).copyWith(fontSize: 20),
        ),
        const SizedBox(height: 4),
        SizedBox(
          width: 350,
          child: Text(
            '${permission.name} needs your permission before Cross Device can pass keystrokes between computers.',
            style: FlowType.body(c.text2).copyWith(fontSize: 13, height: 1.5),
            textAlign: TextAlign.center,
          ),
        ),
        const SizedBox(height: 10),
        Container(
          width: double.infinity,
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
          decoration: BoxDecoration(
            color: c.mat1,
            borderRadius: BorderRadius.circular(12),
          ),
          child: Row(
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      permission.name,
                      style: FlowType.body(c.text1, weight: FontWeight.w600),
                    ),
                    Text(
                      granted ? 'Granted' : 'Not granted yet',
                      style: FlowType.meta(c.text2),
                    ),
                  ],
                ),
              ),
              FlowButton(
                label: granted ? 'Granted' : 'Allow',
                kind: FlowButtonKind.primary,
                background: granted ? c.mat2 : c.accent,
                foreground: granted ? c.text2 : c.accentText,
                onPressed: granted ? null : onGrant,
              ),
            ],
          ),
        ),
        const SizedBox(height: 4),
        FlowButton(
          label: 'Continue',
          kind: FlowButtonKind.primary,
          background: granted ? c.accent : c.mat2,
          foreground: granted ? c.accentText : c.text3,
          large: true,
          onPressed: granted ? onContinue : null,
        ),
      ],
    );
  }
}
