import 'package:flutter/material.dart';

import '../../../core/theme/tokens.dart';
import '../../../core/widgets/primitives.dart';
import '../../../domain/pairing.dart';

/// Step 2 — pairing, reusing the exact same [PairingSession] the tray
/// popover's pairing flow uses (`todos.json` O3): the daemon has one
/// pairing state, not one per surface that shows it. Rendered as a
/// single centered card rather than a list row, per the source's
/// `obStage`/`permCard` treatment.
class PairStep extends StatelessWidget {
  const PairStep({
    super.key,
    required this.palette,
    required this.session,
    required this.onPair,
    required this.onSkip,
  });

  final FlowPalette palette;
  final PairingSession session;
  final ValueChanged<String> onPair;

  /// Leaves onboarding without pairing now — pairing is always available
  /// later from the dashboard, and a user with only one machine set up so
  /// far must not be trapped on this step.
  final VoidCallback onSkip;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          'Find your other computer',
          style: FlowType.heroTitle(c.text1).copyWith(fontSize: 20),
        ),
        const SizedBox(height: 4),
        SizedBox(
          width: 350,
          child: Text(
            'Pairing happens once. After that your computers recognise each other automatically.',
            style: FlowType.body(c.text2).copyWith(fontSize: 13, height: 1.5),
            textAlign: TextAlign.center,
          ),
        ),
        const SizedBox(height: 10),
        switch (session.stage) {
          PairingStage.found when session.candidates.isNotEmpty => Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              for (final candidate in session.candidates)
                _CandidateCard(
                  palette: c,
                  name: candidate.name,
                  onPair: () => onPair(candidate.id),
                ),
            ],
          ),
          PairingStage.requesting => _StatusCard(
            palette: c,
            icon: FlowSpinner(color: c.accent, trackColor: c.mat2),
            text: 'Waiting for approval on ${session.targetName}…',
          ),
          _ => _StatusCard(
            palette: c,
            icon: FlowSpinner(color: c.accent, trackColor: c.mat2),
            text: 'Searching…',
          ),
        },
        const SizedBox(height: 10),
        Text(
          'Keep Cross Device open on your other computer and press '
          '"Pair a device" there too.',
          style: FlowType.meta(c.text3).copyWith(height: 1.4),
          textAlign: TextAlign.center,
        ),
        const SizedBox(height: 6),
        FlowButton(
          label: 'Set up later',
          kind: FlowButtonKind.ghost,
          background: c.mat1,
          foreground: c.text2,
          onPressed: onSkip,
        ),
      ],
    );
  }
}

class _StatusCard extends StatelessWidget {
  const _StatusCard({
    required this.palette,
    required this.icon,
    required this.text,
  });

  final FlowPalette palette;
  final Widget icon;
  final String text;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      decoration: BoxDecoration(
        color: c.mat1,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Row(
        children: [
          icon,
          const SizedBox(width: 12),
          Expanded(child: Text(text, style: FlowType.meta(c.text2))),
        ],
      ),
    );
  }
}

class _CandidateCard extends StatelessWidget {
  const _CandidateCard({
    required this.palette,
    required this.name,
    required this.onPair,
  });

  final FlowPalette palette;
  final String name;
  final VoidCallback onPair;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    return Container(
      width: double.infinity,
      margin: const EdgeInsets.only(bottom: 8),
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      decoration: BoxDecoration(
        color: c.mat1,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Row(
        children: [
          StatusDot(color: c.text2, filled: false),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  name,
                  style: FlowType.body(c.text1, weight: FontWeight.w600),
                ),
                Text('Nearby', style: FlowType.meta(c.text2)),
              ],
            ),
          ),
          FlowButton(
            label: 'Pair',
            kind: FlowButtonKind.primary,
            background: c.accent,
            foreground: c.accentText,
            onPressed: onPair,
          ),
        ],
      ),
    );
  }
}
