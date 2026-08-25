import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/theme/tokens.dart';
import '../../core/widgets/primitives.dart';
import '../../domain/pairing.dart';
import '../../state/repository_providers.dart';

/// The pairing sub-flow within the tray popover — replaces the main
/// header/menu while a [PairingSession] is active. Stage transitions are
/// driven entirely by [pairingSessionProvider]; this widget only issues
/// commands (`cancelPairing`, `pairWithCandidate`), it never times
/// anything itself (`docs/contracts/daemon-ipc.md` owns the timing).
class TrayPairingView extends ConsumerWidget {
  const TrayPairingView({
    super.key,
    required this.session,
    required this.palette,
    required this.switchKeyLabel,
  });

  final PairingSession session;
  final FlowPalette palette;
  final String switchKeyLabel;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = palette;
    return Padding(
      padding: const EdgeInsets.only(bottom: 9),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 13, 14, 11),
            child: Row(
              children: [
                GestureDetector(
                  onTap: () =>
                      ref.read(daemonRepositoryProvider).cancelPairing(),
                  child: Container(
                    width: 22,
                    height: 22,
                    decoration: BoxDecoration(
                      color: c.mat1,
                      borderRadius: BorderRadius.circular(7),
                    ),
                    alignment: Alignment.center,
                    child: Text(
                      '‹',
                      style: TextStyle(fontSize: 15, color: c.text2),
                    ),
                  ),
                ),
                const SizedBox(width: 10),
                Text(
                  'Pair New Device',
                  style: FlowType.body(
                    c.text1,
                    weight: FontWeight.w700,
                  ).copyWith(fontSize: 13.5),
                ),
              ],
            ),
          ),
          switch (session.stage) {
            PairingStage.searching => _CenterMessage(
              palette: c,
              icon: FlowSpinner(color: c.accent, trackColor: c.mat2),
              title: 'Searching for devices…',
              hint: 'Keep Cross Device open on your other computer.',
            ),
            PairingStage.found => _CandidateList(session: session, palette: c),
            PairingStage.requesting => _CenterMessage(
              palette: c,
              icon: FlowSpinner(color: c.accent, trackColor: c.mat2),
              title: 'Waiting for approval…',
              hint: 'Approve the request on ${session.targetName}.',
            ),
            PairingStage.paired => _CenterMessage(
              palette: c,
              icon: Container(
                width: 34,
                height: 34,
                decoration: const BoxDecoration(
                  color: Color(0xFF30D158),
                  shape: BoxShape.circle,
                ),
                alignment: Alignment.center,
                child: const Text(
                  '✓',
                  style: TextStyle(color: Colors.white, fontSize: 16),
                ),
              ),
              title: '${session.targetName} connected',
              hint: 'Press $switchKeyLabel to switch to it.',
            ),
            PairingStage.idle || PairingStage.failed => const SizedBox.shrink(),
          },
        ],
      ),
    );
  }
}

class _CenterMessage extends StatelessWidget {
  const _CenterMessage({
    required this.palette,
    required this.icon,
    required this.title,
    required this.hint,
  });

  final FlowPalette palette;
  final Widget icon;
  final String title;
  final String hint;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    return Padding(
      padding: const EdgeInsets.fromLTRB(24, 20, 24, 26),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          icon,
          const SizedBox(height: 9),
          Text(
            title,
            style: FlowType.body(
              c.text1,
              weight: FontWeight.w600,
            ).copyWith(fontSize: 13.5),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: 4),
          Text(
            hint,
            style: FlowType.meta(c.text2).copyWith(height: 1.4),
            textAlign: TextAlign.center,
          ),
        ],
      ),
    );
  }
}

class _CandidateList extends ConsumerWidget {
  const _CandidateList({required this.session, required this.palette});

  final PairingSession session;
  final FlowPalette palette;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = palette;
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        for (final candidate in session.candidates)
          Container(
            margin: const EdgeInsets.fromLTRB(8, 0, 8, 3),
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
            decoration: BoxDecoration(
              color: c.mat1,
              borderRadius: BorderRadius.circular(10),
            ),
            child: Row(
              children: [
                StatusDot(color: c.text2, filled: false),
                const SizedBox(width: 11),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(
                        candidate.name,
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
                  onPressed: () => ref
                      .read(daemonRepositoryProvider)
                      .pairWithCandidate(candidate.id),
                ),
              ],
            ),
          ),
        Padding(
          padding: const EdgeInsets.fromLTRB(20, 4, 20, 6),
          child: Text(
            'Only devices signed in as you appear here.',
            style: FlowType.meta(c.text3).copyWith(height: 1.4),
          ),
        ),
      ],
    );
  }
}
