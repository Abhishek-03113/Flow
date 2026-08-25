import 'package:flutter/material.dart';

import '../platform_chrome.dart';

/// The 40px window title bar, in the three variants keyed off
/// [ChromeControls] — direct port of `WindowChrome.dc.html`'s three
/// branches. Colors are supplied by the caller (from the active
/// [FlowPalette]) rather than hardcoded, so this one widget works in
/// both themes and doesn't need to know about [FlowColors] itself.
class WindowChrome extends StatelessWidget {
  const WindowChrome({
    super.key,
    required this.controls,
    required this.title,
    required this.background,
    required this.border,
    required this.textColor,
    required this.textSecondary,
  });

  final ChromeControls controls;
  final String title;
  final Color background;
  final Color border;
  final Color textColor;
  final Color textSecondary;

  @override
  Widget build(BuildContext context) {
    final titleWidget = Text(
      title,
      style: TextStyle(
        fontSize: 13,
        fontWeight: FontWeight.w600,
        color: textColor,
      ),
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      textAlign: controls == ChromeControls.win
          ? TextAlign.left
          : TextAlign.center,
    );

    return Container(
      height: PlatformChrome.windowChromeBarHeight,
      padding: const EdgeInsets.symmetric(horizontal: 14),
      decoration: BoxDecoration(
        color: background,
        border: Border(bottom: BorderSide(color: border)),
        borderRadius: const BorderRadius.only(
          topLeft: Radius.circular(10),
          topRight: Radius.circular(10),
        ),
      ),
      child: Row(
        children: [
          if (controls == ChromeControls.mac) _MacDots(),
          if (controls == ChromeControls.mac) const SizedBox(width: 10),
          Expanded(child: titleWidget),
          if (controls != ChromeControls.mac) const SizedBox(width: 10),
          if (controls == ChromeControls.win)
            _WinControls(color: textSecondary),
          if (controls == ChromeControls.gnome)
            _GnomeControls(color: textSecondary),
        ],
      ),
    );
  }
}

class _MacDots extends StatelessWidget {
  const _MacDots();

  @override
  Widget build(BuildContext context) {
    Widget dot(Color color) => Container(
      width: 11,
      height: 11,
      decoration: BoxDecoration(color: color, shape: BoxShape.circle),
    );
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        dot(const Color(0xFFFF5F57)),
        const SizedBox(width: 7),
        dot(const Color(0xFFFEBC2E)),
        const SizedBox(width: 7),
        dot(const Color(0xFF28C840)),
      ],
    );
  }
}

class _WinControls extends StatelessWidget {
  const _WinControls({required this.color});

  final Color color;

  @override
  Widget build(BuildContext context) {
    Widget btn(String glyph) => SizedBox(
      width: 32,
      height: 40,
      child: Center(
        child: Text(glyph, style: TextStyle(fontSize: 13, color: color)),
      ),
    );
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [btn('—'), btn('□'), btn('✕')],
    );
  }
}

class _GnomeControls extends StatelessWidget {
  const _GnomeControls({required this.color});

  final Color color;

  @override
  Widget build(BuildContext context) {
    Widget btn(String glyph) => Container(
      width: 20,
      height: 20,
      alignment: Alignment.center,
      decoration: const BoxDecoration(
        color: Color(0x2E808080),
        shape: BoxShape.circle,
      ),
      child: Text(glyph, style: TextStyle(fontSize: 11, color: color)),
    );
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        btn('–'),
        const SizedBox(width: 8),
        btn('□'),
        const SizedBox(width: 8),
        btn('✕'),
      ],
    );
  }
}
