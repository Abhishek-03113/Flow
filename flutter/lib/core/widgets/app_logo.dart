import 'package:flutter/material.dart';

/// The Flow app mark. Renders `assets/flow_logo.png` once that file exists
/// in the repo; until then (and if it's ever missing/corrupt) it falls
/// back to the accent rounded square onboarding always showed, so this
/// widget is safe to drop in everywhere branding is needed without a
/// second code change once the real asset lands.
class AppLogo extends StatelessWidget {
  const AppLogo({super.key, required this.accent, this.size = 58});

  final Color accent;
  final double size;

  @override
  Widget build(BuildContext context) {
    final radius = size * 17 / 58;
    return Container(
      width: size,
      height: size,
      clipBehavior: Clip.antiAlias,
      decoration: BoxDecoration(
        color: accent,
        borderRadius: BorderRadius.circular(radius),
        boxShadow: [
          BoxShadow(
            color: accent.withValues(alpha: 0.35),
            blurRadius: size * 0.4,
            offset: Offset(0, size * 0.14),
          ),
        ],
      ),
      child: Image.asset(
        'assets/flow_logo.png',
        fit: BoxFit.cover,
        errorBuilder: (context, error, stackTrace) => const SizedBox.shrink(),
      ),
    );
  }
}

/// A small inline mark (tray/menu headers, step chrome) — same fallback
/// behavior as [AppLogo], just sized and shaped for a tight row instead of
/// a hero moment.
class AppLogoMark extends StatelessWidget {
  const AppLogoMark({super.key, required this.accent, this.size = 20});

  final Color accent;
  final double size;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: size,
      height: size,
      clipBehavior: Clip.antiAlias,
      decoration: BoxDecoration(
        color: accent,
        borderRadius: BorderRadius.circular(size * 0.32),
      ),
      child: Image.asset(
        'assets/flow_logo.png',
        fit: BoxFit.cover,
        errorBuilder: (context, error, stackTrace) => const SizedBox.shrink(),
      ),
    );
  }
}
