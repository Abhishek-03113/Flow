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
/// `WindowChrome` + a 176px sidebar + a content area that swaps sections.
/// Direct implementation of the `isApp` branch in `Cross-Device UI v2.
/// dc.html`.
class AppWindowShell extends ConsumerStatefulWidget {
  const AppWindowShell({
    super.key,
    required this.platform,
    this.initialSection = AppSection.dashboard,
  });

  final HostOs platform;
  final AppSection initialSection;

  @override
  ConsumerState<AppWindowShell> createState() => _AppWindowShellState();
}

class _AppWindowShellState extends ConsumerState<AppWindowShell> {
  late AppSection _section = widget.initialSection;

  @override
  Widget build(BuildContext context) {
    final c = FlowColors.of(context);
    final chrome = PlatformChrome.of(widget.platform);

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
            WindowChrome(
              controls: chrome.controls,
              title: _section == AppSection.dashboard
                  ? 'Cross Device'
                  : 'Settings',
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
