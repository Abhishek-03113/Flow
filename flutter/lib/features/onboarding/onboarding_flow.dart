import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/platform_chrome.dart';
import '../../core/theme/flow_theme.dart';
import '../../core/theme/tokens.dart';
import '../../core/widgets/glass_surface.dart';
import '../../core/widgets/window_chrome.dart';
import '../../domain/device.dart';
import '../../domain/pairing.dart';
import '../../domain/permission_status.dart';
import '../../state/repository_providers.dart';
import 'steps/done_step.dart';
import 'steps/pair_step.dart';
import 'steps/permission_step.dart';
import 'steps/welcome_step.dart';

/// The 4-step first-launch window (welcome, permission, pair, done),
/// direct implementation of the `isOnboarding` branch in `Cross-Device
/// UI v2.dc.html`: a 520px glass window with `WindowChrome`, animated
/// step transitions, and dot pagination.
class OnboardingFlow extends ConsumerStatefulWidget {
  const OnboardingFlow({
    super.key,
    required this.platform,
    required this.onDone,
  });

  final HostOs platform;
  final VoidCallback onDone;

  @override
  ConsumerState<OnboardingFlow> createState() => _OnboardingFlowState();
}

class _OnboardingFlowState extends ConsumerState<OnboardingFlow> {
  int _step = 0;

  void _goTo(int step) {
    setState(() => _step = step);
    if (step == 2) {
      final session = ref.read(pairingSessionProvider).valueOrNull;
      if (session == null || session.stage == PairingStage.idle) {
        ref.read(daemonRepositoryProvider).startPairing();
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = FlowColors.of(context);
    final chrome = PlatformChrome.of(widget.platform);

    // Once pairing succeeds, move on to the Done step automatically —
    // onboarding doesn't need to wait for the pairing session's own
    // auto-return-to-idle timer the way the tray popover does.
    ref.listen(pairingSessionProvider, (previous, next) {
      final stage = next.valueOrNull?.stage;
      if (_step == 2 && stage == PairingStage.paired) {
        setState(() => _step = 3);
      }
    });

    final permission = ref.watch(permissionProvider).valueOrNull;
    final session =
        ref.watch(pairingSessionProvider).valueOrNull ?? PairingSession.idle;
    final switchKeyLabel =
        ref.watch(settingsProvider).valueOrNull?.switchKey.label ?? '—';

    return GlassSurface(
      color: c.winGlass,
      border: c.border,
      blurSigma: 40,
      borderRadius: BorderRadius.circular(chrome.windowRadius()),
      boxShadow: c.shadow,
      child: SizedBox(
        width: 520,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            WindowChrome(
              controls: chrome.controls,
              title: 'Cross Device',
              background: c.chromeBg,
              border: c.border,
              textColor: c.text1,
              textSecondary: c.text2,
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(34, 30, 34, 22),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  AnimatedSwitcher(
                    duration: const Duration(milliseconds: 240),
                    transitionBuilder: (child, animation) => FadeTransition(
                      opacity: animation,
                      child: SlideTransition(
                        position: animation.drive(
                          Tween(begin: const Offset(0, 0.02), end: Offset.zero),
                        ),
                        child: child,
                      ),
                    ),
                    child: KeyedSubtree(
                      key: ValueKey(_step),
                      child: switch (_step) {
                        0 => WelcomeStep(
                          palette: c,
                          onContinue: () => _goTo(1),
                        ),
                        1 => PermissionStep(
                          palette: c,
                          permission:
                              permission ??
                              PermissionStatus(
                                name: chrome.permissionName,
                                granted: false,
                              ),
                          onGrant: () => ref
                              .read(daemonRepositoryProvider)
                              .requestPermission(),
                          onContinue: () => _goTo(2),
                        ),
                        2 => PairStep(
                          palette: c,
                          session: session,
                          onPair: (id) => ref
                              .read(daemonRepositoryProvider)
                              .pairWithCandidate(id),
                        ),
                        _ => DoneStep(
                          palette: c,
                          trayName: chrome.trayName,
                          switchKeyLabel: switchKeyLabel,
                          onDone: widget.onDone,
                        ),
                      },
                    ),
                  ),
                  const SizedBox(height: 18),
                  Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      for (var i = 0; i < 4; i++)
                        AnimatedContainer(
                          duration: FlowMotion.hoverPress,
                          curve: FlowMotion.ease,
                          margin: const EdgeInsets.symmetric(horizontal: 3),
                          width: 5,
                          height: 5,
                          decoration: BoxDecoration(
                            color: i == _step ? c.accent : c.mat3,
                            shape: BoxShape.circle,
                          ),
                        ),
                    ],
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
