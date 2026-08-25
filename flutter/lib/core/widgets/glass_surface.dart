import 'dart:ui';

import 'package:flutter/material.dart';

/// The translucent "glass" material behind the tray popover, onboarding
/// window, app window, and toast — a blurred, tinted, bordered container
/// with an optional inset top highlight. One widget reused by all four
/// instead of four bespoke containers with copy-pasted `BackdropFilter`s.
class GlassSurface extends StatelessWidget {
  const GlassSurface({
    super.key,
    required this.child,
    required this.color,
    required this.border,
    this.blurSigma = 40,
    this.borderRadius = const BorderRadius.all(Radius.circular(14)),
    this.boxShadow,
    this.insetHighlight = false,
  });

  final Widget child;
  final Color color;
  final Color border;
  final double blurSigma;
  final BorderRadius borderRadius;
  final List<BoxShadow>? boxShadow;

  /// A faint top inset highlight (`inset 0 1px 0 rgba(255,255,255,...)`
  /// in the source), used on the tray popover.
  final bool insetHighlight;

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: BoxDecoration(
        borderRadius: borderRadius,
        boxShadow: boxShadow,
      ),
      child: ClipRRect(
        borderRadius: borderRadius,
        child: BackdropFilter(
          filter: ImageFilter.blur(sigmaX: blurSigma, sigmaY: blurSigma),
          child: Container(
            decoration: BoxDecoration(
              color: color,
              borderRadius: borderRadius,
              border: Border.all(color: border, width: 1),
              boxShadow: insetHighlight
                  ? const [
                      BoxShadow(
                        color: Color(0x17FFFFFF),
                        blurRadius: 0,
                        spreadRadius: -0.5,
                        offset: Offset(0, 1),
                      ),
                    ]
                  : null,
            ),
            child: child,
          ),
        ),
      ),
    );
  }
}
