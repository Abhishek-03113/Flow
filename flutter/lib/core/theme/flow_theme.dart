import 'package:flutter/material.dart';

import 'tokens.dart';

/// Makes [FlowPalette] reachable via `Theme.of(context)` alongside the
/// rest of Flutter's theme, instead of threading tokens through every
/// widget constructor by hand.
class FlowColors extends ThemeExtension<FlowColors> {
  const FlowColors(this.palette);

  final FlowPalette palette;

  @override
  FlowColors copyWith({FlowPalette? palette}) =>
      FlowColors(palette ?? this.palette);

  @override
  FlowColors lerp(ThemeExtension<FlowColors>? other, double t) {
    // The design switches themes instantly (a segmented-control tap), not
    // via a cross-fade — identity lerp is the correct behavior here, not
    // a missing feature.
    if (other is! FlowColors) return this;
    return t < 0.5 ? this : other;
  }

  static FlowPalette of(BuildContext context) {
    return Theme.of(context).extension<FlowColors>()!.palette;
  }
}

class FlowTheme {
  const FlowTheme._();

  static ThemeData dark() => _build(FlowPalette.dark, Brightness.dark);
  static ThemeData light() => _build(FlowPalette.light, Brightness.light);

  static ThemeData _build(FlowPalette palette, Brightness brightness) {
    return ThemeData(
      brightness: brightness,
      scaffoldBackgroundColor: palette.shell,
      colorScheme: ColorScheme.fromSeed(
        seedColor: palette.accent,
        brightness: brightness,
      ),
      extensions: [FlowColors(palette)],
    );
  }
}
