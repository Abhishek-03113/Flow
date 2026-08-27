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
    this.isRequesting = false,
    this.errorMessage,
    this.connectionError = false,
    this.allowSkip = false,
  });

  final FlowPalette palette;
  final PermissionStatus permission;
  final VoidCallback onGrant;
  final VoidCallback onContinue;

  /// True in local development (`--dart-define=FLOW_ENV=development`, see
  /// `state/app_env.dart`) — lets "Continue" proceed without the
  /// permission actually granted. macOS only grants Accessibility to a
  /// stable, installed app bundle, so a `flutter run` build can never
  /// satisfy this gate no matter what the user clicks; blocking onboarding
  /// on it there would make the rest of the app unreachable in dev.
  final bool allowSkip;

  /// True while the daemon command triggered by "Allow" is in flight —
  /// shows a spinner and disables the button instead of leaving it looking
  /// clickable with nothing visibly happening.
  final bool isRequesting;

  /// Set after a failed request. Shown inline so getting stuck here has an
  /// actual explanation instead of just a button that appears to do
  /// nothing.
  final String? errorMessage;

  /// True when [permission] is still unknown because `flow-daemon` itself
  /// couldn't be reached at all (as opposed to "reachable, just not
  /// granted yet") — a distinct, more actionable message than the generic
  /// "Not granted yet" copy.
  final bool connectionError;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    final granted = permission.granted;
    final canContinue = granted || allowSkip;
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
            '${permission.name} needs your permission before Cross Device '
            "can pass keystrokes between computers. Allow opens your "
            "system's privacy settings — flip the switch there, then come "
            'back here.',
            style: FlowType.body(c.text2).copyWith(fontSize: 13, height: 1.5),
            textAlign: TextAlign.center,
          ),
        ),
        const SizedBox(height: 10),
        if (connectionError)
          Container(
            width: double.infinity,
            margin: const EdgeInsets.only(bottom: 8),
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
            decoration: BoxDecoration(
              color: c.dangerSoft,
              borderRadius: BorderRadius.circular(12),
            ),
            child: Text(
              "Can't reach Flow's background service. Make sure it's "
              'running, then tap Allow to try again.',
              style: FlowType.meta(c.danger),
            ),
          ),
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
              if (isRequesting)
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 10),
                  child: FlowSpinner(color: c.accent, trackColor: c.mat2),
                )
              else
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
        if (errorMessage != null)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(
              errorMessage!,
              style: FlowType.meta(c.danger),
              textAlign: TextAlign.center,
            ),
          ),
        if (allowSkip && !granted)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(
              "Development build — Continue won't wait on this permission "
              "since macOS can't grant it until the app is installed.",
              style: FlowType.meta(c.text3),
              textAlign: TextAlign.center,
            ),
          ),
        const SizedBox(height: 4),
        FlowButton(
          label: 'Continue',
          kind: FlowButtonKind.primary,
          background: canContinue ? c.accent : c.mat2,
          foreground: canContinue ? c.accentText : c.text3,
          large: true,
          onPressed: canContinue ? onContinue : null,
        ),
      ],
    );
  }
}
