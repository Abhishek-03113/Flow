import 'package:flutter/material.dart';

/// Raw design values from the Claude Design source (`todos.json`'s
/// `sharedDesignTokens`), translated to Flutter types. This file has no
/// opinion on *how* they're assembled into a theme — see `flow_theme.dart`
/// for that; this is just the vocabulary.

/// The full color palette for one brightness. Two instances exist:
/// [FlowPalette.dark] and [FlowPalette.light].
///
/// Field names match `docs/contracts`-adjacent design source names
/// (`mat1`/`mat2`/`mat3` are stacked "material" overlay tiers, `t1`/`t2`/
/// `t3` are stacked text-opacity tiers), not generic Material color-role
/// names — this app's surfaces don't map cleanly onto Material's roles,
/// so renaming them to fit would lose information rather than add it.
class FlowPalette {
  const FlowPalette({
    required this.border,
    required this.chromeBg,
    required this.winGlass,
    required this.trayGlass,
    required this.mat1,
    required this.mat2,
    required this.mat3,
    required this.text1,
    required this.text2,
    required this.text3,
    required this.accent,
    required this.accentSoft,
    required this.accentText,
    required this.barBg,
    required this.hairline,
    required this.shell,
    required this.danger,
    required this.dangerSoft,
    required this.shadow,
    required this.statusActive,
    required this.statusIdle,
    required this.statusPending,
    required this.statusError,
    required this.statusOffline,
    required this.wallpaper,
  });

  final Color border;
  final Color chromeBg;
  final Color winGlass;

  /// `TrayPopover.dc.html`'s own `glass` token — a distinct tint from
  /// [winGlass] used only by the tray popover, not the onboarding/app
  /// windows.
  final Color trayGlass;
  final Color mat1;
  final Color mat2;
  final Color mat3;
  final Color text1;
  final Color text2;
  final Color text3;
  final Color accent;
  final Color accentSoft;
  final Color accentText;
  final Color barBg;
  final Color hairline;
  final Color shell;
  final Color danger;
  final Color dangerSoft;
  final List<BoxShadow> shadow;
  final Color statusActive;
  final Color statusIdle;
  final Color statusPending;
  final Color statusError;

  /// `#8e8e93` in the source — the `disconnected` link state's dot
  /// color, distinct from the four semantic status colors above (also
  /// fixed regardless of theme).
  final Color statusOffline;

  /// Layered radial-gradient wallpaper behind the desktop/menu-bar mock.
  final Gradient wallpaper;

  static const dark = FlowPalette(
    border: Color(0x24FFFFFF), // rgba(255,255,255,0.14)
    chromeBg: Color(0xD1282828), // rgba(40,40,44,0.82)
    winGlass: Color(0xCC1A1A1E), // rgba(26,26,30,0.8)
    trayGlass: Color(0xAD1C1C20), // rgba(28,28,32,0.68)
    mat1: Color(0x0EFFFFFF), // rgba(255,255,255,0.055)
    mat2: Color(0x1AFFFFFF), // rgba(255,255,255,0.1)
    mat3: Color(0x24FFFFFF), // rgba(255,255,255,0.14)
    text1: Color(0xF0FFFFFF), // rgba(255,255,255,0.94)
    text2: Color(0x8FFFFFFF), // rgba(255,255,255,0.56)
    text3: Color(0x5CFFFFFF), // rgba(255,255,255,0.36)
    accent: Color(0xFF3496EF), // oklch(0.66 0.16 250)
    accentSoft: Color(0x286E8CFF), // rgba(110,140,255,0.16)
    accentText: Color(0xFFFFFFFF),
    barBg: Color(0x99101014), // rgba(16,16,20,0.6)
    hairline: Color(0x17FFFFFF), // rgba(255,255,255,0.09)
    shell: Color(0xFF0B0C0F),
    danger: Color(0xFFFF453A),
    dangerSoft: Color(0x24FF453A), // rgba(255,69,58,0.14)
    shadow: [
      BoxShadow(
        color: Color(0x80000000),
        blurRadius: 64,
        offset: Offset(0, 24),
      ),
      BoxShadow(color: Color(0x4D000000), blurRadius: 8, offset: Offset(0, 2)),
    ],
    statusActive: Color(0xFF30D158),
    statusIdle: Color(0xFF0A84FF),
    statusPending: Color(0xFFFF9F0A),
    statusError: Color(0xFFFF453A),
    statusOffline: Color(0xFF8E8E93),
    wallpaper: RadialGradient(
      center: Alignment(-0.7, -0.8),
      radius: 1.3,
      colors: [Color(0xFF2B3550), Color(0xFF101319)],
    ),
  );

  static const light = FlowPalette(
    border: Color(0x17000000), // rgba(0,0,0,0.09)
    chromeBg: Color(0xE0F6F6F9), // rgba(246,246,249,0.88)
    winGlass: Color(0xDBFAFAFC), // rgba(250,250,252,0.86)
    trayGlass: Color(0xBDFCFCFE), // rgba(252,252,254,0.74)
    mat1: Color(0x0C0A0C14), // rgba(10,12,20,0.045)
    mat2: Color(0x130A0C14), // rgba(10,12,20,0.075)
    mat3: Color(0x1A0A0C14), // rgba(10,12,20,0.1)
    text1: Color(0xE0000000), // rgba(0,0,0,0.88)
    text2: Color(0x80000000), // rgba(0,0,0,0.5)
    text3: Color(0x57000000), // rgba(0,0,0,0.34)
    accent: Color(0xFF0072D5), // oklch(0.55 0.18 250)
    accentSoft: Color(0xFFDDEDFF), // oklch(0.94 0.03 250)
    accentText: Color(0xFFFFFFFF),
    barBg: Color(0x8CFFFFFF), // rgba(255,255,255,0.55)
    hairline: Color(0x14000000), // rgba(0,0,0,0.08)
    shell: Color(0xFFEEF0F4),
    danger: Color(0xFFD92D20),
    dangerSoft: Color(0x1AD92D20), // rgba(217,45,32,0.1)
    shadow: [
      BoxShadow(
        color: Color(0x33141828),
        blurRadius: 60,
        offset: Offset(0, 24),
      ),
      BoxShadow(color: Color(0x1A141828), blurRadius: 6, offset: Offset(0, 2)),
    ],
    statusActive: Color(0xFF30D158),
    statusIdle: Color(0xFF0A84FF),
    statusPending: Color(0xFFFF9F0A),
    // Status dot colors are fixed hex in the design source (TrayPopover.
    // dc.html's STATE_META, and the main file's activeDotBig/dotS
    // defaults) regardless of theme — unlike `danger`/`dangerSoft`, which
    // genuinely do vary per theme for generic error chrome (buttons,
    // banners). #D92D20 here would have been borrowing the theme-varying
    // `danger` value for a token the source never varies.
    statusError: Color(0xFFFF453A),
    statusOffline: Color(0xFF8E8E93),
    wallpaper: RadialGradient(
      center: Alignment(-0.6, -0.9),
      radius: 1.3,
      colors: [Color(0xFFCFD9EF), Color(0xFFE9ECF2)],
    ),
  );
}

/// Corner radii, keyed to what they're for rather than a generic T-shirt
/// scale, because the design assigns radius by platform for windows/
/// popovers and doesn't otherwise use a uniform scale.
class FlowRadii {
  const FlowRadii._();

  // Window radius is popover radius + 2px in the source (onboardWindow/
  // appWindow both use `plat.radius + 2`) — kept as separate named
  // constants rather than derived arithmetic so both read as intentional
  // design values, not an accident of one being computed from the other.
  static const macPopover = 16.0;
  static const windowsPopover = 10.0;
  static const linuxPopover = 12.0;
  static const macWindow = 18.0;
  static const windowsWindow = 12.0;
  static const linuxWindow = 14.0;
  static const card = 12.0;
  static const button = 8.0;
  static const toggleTrack = 11.0;
}

/// Animation durations and curves used across the design (`cd-pop`,
/// `cd-fade`, `cd-spin`, `cd-breathe`, and the default hover/press ease).
class FlowMotion {
  const FlowMotion._();

  static const ease = Cubic(0.32, 0.72, 0, 1);
  static const hoverPress = Duration(milliseconds: 180);
  static const popoverEnter = Duration(milliseconds: 220);
  static const fadeIn = Duration(milliseconds: 220);
  static const spin = Duration(milliseconds: 800);
  static const breathe = Duration(milliseconds: 1350);
}

/// Type scale as [TextStyle] factories rather than fixed constants, since
/// every use also needs a palette color the caller already has in hand.
class FlowType {
  const FlowType._();

  static const _font = [
    'SF Pro Text',
    'Segoe UI Variable',
    'Segoe UI',
    'Roboto',
    'Helvetica Neue',
  ];

  static TextStyle sectionLabel(Color color) => TextStyle(
    fontFamilyFallback: _font,
    fontSize: 10.5,
    fontWeight: FontWeight.w700,
    letterSpacing: 0.7,
    color: color,
  );

  static TextStyle body(Color color, {FontWeight weight = FontWeight.w500}) =>
      TextStyle(
        fontFamilyFallback: _font,
        fontSize: 13,
        fontWeight: weight,
        color: color,
        letterSpacing: -0.1,
      );

  static TextStyle meta(Color color) => TextStyle(
    fontFamilyFallback: _font,
    fontSize: 11.5,
    color: color,
    letterSpacing: -0.1,
  );

  static TextStyle heroTitle(Color color) => TextStyle(
    fontFamilyFallback: _font,
    fontSize: 22,
    fontWeight: FontWeight.w700,
    letterSpacing: -0.4,
    color: color,
  );

  static TextStyle buttonLabel(
    Color color, {
    FontWeight weight = FontWeight.w600,
  }) => TextStyle(
    fontFamilyFallback: _font,
    fontSize: 12.5,
    fontWeight: weight,
    color: color,
  );
}
