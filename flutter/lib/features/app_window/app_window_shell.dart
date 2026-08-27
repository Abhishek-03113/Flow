import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/platform_chrome.dart';
import '../../core/theme/flow_theme.dart';
import '../../core/theme/tokens.dart';
import '../../core/widgets/glass_surface.dart';
import '../../core/widgets/window_chrome.dart';
import '../../domain/device.dart';
import 'sections/advanced_section.dart';
import 'sections/dashboard_section.dart';
import 'sections/devices_section.dart';
import 'sections/general_section.dart';
import 'sections/input_section.dart';

/// Which content the app window shows. `dashboard` is the "Overview"
/// group; the rest are the "Settings" group.
enum AppSection { dashboard, general, devices, input, advanced }

/// The 720x470 window opened from the tray's Dashboard/Settings rows:
/// a title bar + a 176px sidebar + a content area that swaps sections.
/// Direct implementation of the `isApp` branch in `Cross-Device UI v2.
/// dc.html`.
///
/// [standalone] switches the title bar the same way
/// `OnboardingFlow.standalone` does (see that class's doc comment for the
/// full rationale): `false` (the dev harness, which places this on top of
/// a mock desktop wallpaper with no real OS window backing it) keeps the
/// decorative `WindowChrome` with its own fake traffic lights/title bar;
/// `true` (the shipped app's `_RealApp`, which already hosts this inside
/// a real OS window with its own real title bar) swaps it for a plain
/// text header instead, so the Dashboard/Settings window stops rendering
/// a second, fake window chrome nested inside the real one.
class AppWindowShell extends ConsumerStatefulWidget {
  const AppWindowShell({
    super.key,
    required this.platform,
    this.initialSection = AppSection.dashboard,
    this.standalone = false,
  });

  final HostOs platform;
  final AppSection initialSection;
  final bool standalone;

  @override
  ConsumerState<AppWindowShell> createState() => _AppWindowShellState();
}

class _AppWindowShellState extends ConsumerState<AppWindowShell> {
  late AppSection _section = widget.initialSection;

  @override
  Widget build(BuildContext context) {
    final c = FlowColors.of(context);
    final chrome = PlatformChrome.of(widget.platform);
    final title = _section == AppSection.dashboard
        ? 'Cross Device'
        : 'Settings';

    return GlassSurface(
      color: c.winGlass,
      border: c.border,
      blurSigma: 40,
      borderRadius: BorderRadius.circular(chrome.windowRadius()),
      boxShadow: c.shadow,
      child: SizedBox(
        width: 720,
        height: 470,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            widget.standalone
                ? _StandaloneTitleHeader(palette: c, title: title)
                : WindowChrome(
                    controls: chrome.controls,
                    title: title,
                    background: c.chromeBg,
                    border: c.border,
                    textColor: c.text1,
                    textSecondary: c.text2,
                  ),
            Expanded(
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  _Sidebar(
                    palette: c,
                    section: _section,
                    onSelect: (s) => setState(() => _section = s),
                  ),
                  Expanded(
                    child: SingleChildScrollView(
                      padding: const EdgeInsets.fromLTRB(28, 24, 28, 24),
                      child: switch (_section) {
                        AppSection.dashboard => DashboardSection(palette: c),
                        AppSection.general => GeneralSection(
                          palette: c,
                          chrome: chrome,
                        ),
                        AppSection.devices => DevicesSection(palette: c),
                        AppSection.input => InputSection(
                          palette: c,
                          platform: widget.platform,
                        ),
                        AppSection.advanced => AdvancedSection(palette: c),
                      },
                    ),
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

/// [AppWindowShell.standalone]'s title bar: a plain text header replacing
/// [WindowChrome] so the panel stops drawing a second, fake title bar
/// (with its own fake traffic lights) inside `_RealApp`'s real OS window
/// — the same fix `OnboardingFlow`'s own `_StandaloneHeader` applies for
/// onboarding (see that class's doc comment).
class _StandaloneTitleHeader extends StatelessWidget {
  const _StandaloneTitleHeader({required this.palette, required this.title});

  final FlowPalette palette;
  final String title;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    return Container(
      padding: const EdgeInsets.fromLTRB(20, 18, 20, 12),
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: c.hairline)),
      ),
      child: Text(
        title,
        style: FlowType.body(c.text1, weight: FontWeight.w700).copyWith(
          fontSize: 15,
        ),
      ),
    );
  }
}

class _Sidebar extends StatelessWidget {
  const _Sidebar({
    required this.palette,
    required this.section,
    required this.onSelect,
  });

  final FlowPalette palette;
  final AppSection section;
  final ValueChanged<AppSection> onSelect;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    return Container(
      width: 176,
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 14),
      decoration: BoxDecoration(
        border: Border(right: BorderSide(color: c.hairline)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _label('Overview', c),
          _item(c, 'Dashboard', AppSection.dashboard),
          _label('Settings', c),
          _item(c, 'General', AppSection.general),
          _item(c, 'Devices', AppSection.devices),
          _item(c, 'Input', AppSection.input),
          _item(c, 'Advanced', AppSection.advanced),
        ],
      ),
    );
  }

  Widget _label(String text, FlowPalette c) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(10, 10, 10, 4),
      child: Text(
        text,
        style: FlowType.sectionLabel(c.text3).copyWith(fontSize: 10),
      ),
    );
  }

  Widget _item(FlowPalette c, String label, AppSection value) {
    final selected = value == section;
    return GestureDetector(
      onTap: () => onSelect(value),
      child: Container(
        margin: const EdgeInsets.only(bottom: 2),
        padding: const EdgeInsets.symmetric(horizontal: 11, vertical: 8),
        decoration: BoxDecoration(
          color: selected ? c.mat2 : Colors.transparent,
          borderRadius: BorderRadius.circular(9),
        ),
        child: Text(
          label,
          style: FlowType.body(
            selected ? c.text1 : c.text2,
            weight: FontWeight.w600,
          ).copyWith(fontSize: 12.5),
        ),
      ),
    );
  }
}
