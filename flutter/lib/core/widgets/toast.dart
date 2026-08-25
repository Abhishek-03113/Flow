import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../state/ui_providers.dart';
import '../theme/flow_theme.dart';
import '../theme/tokens.dart';
import 'glass_surface.dart';

/// Top-center transient pill notification, positioned by wrapping the
/// screen content in a [Stack] — place this as the last child so it
/// paints above everything else. Reads [toastProvider] directly; nothing
/// else needs to know a toast is showing.
class ToastOverlay extends ConsumerWidget {
  const ToastOverlay({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final message = ref.watch(toastProvider);
    final c = FlowColors.of(context);

    return Positioned(
      top: 18,
      left: 0,
      right: 0,
      child: Center(
        child: IgnorePointer(
          child: AnimatedSwitcher(
            duration: const Duration(milliseconds: 200),
            transitionBuilder: (child, animation) => FadeTransition(
              opacity: animation,
              child: SlideTransition(
                position: animation.drive(
                  Tween(begin: const Offset(0, -0.08), end: Offset.zero),
                ),
                child: child,
              ),
            ),
            child: message == null
                ? const SizedBox.shrink(key: ValueKey('empty'))
                : _ToastPill(
                    key: ValueKey(message),
                    message: message,
                    palette: c,
                  ),
          ),
        ),
      ),
    );
  }
}

class _ToastPill extends StatelessWidget {
  const _ToastPill({super.key, required this.message, required this.palette});

  final String message;
  final FlowPalette palette;

  @override
  Widget build(BuildContext context) {
    return GlassSurface(
      color: palette.winGlass,
      border: palette.border,
      blurSigma: 30,
      borderRadius: BorderRadius.circular(20),
      boxShadow: palette.shadow,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 9),
        child: Text(
          message,
          style: TextStyle(
            fontSize: 12.5,
            fontWeight: FontWeight.w600,
            color: palette.text1,
          ),
        ),
      ),
    );
  }
}
