import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/platform_chrome.dart';
import '../../../core/theme/tokens.dart';
import '../../../core/widgets/primitives.dart';
import '../../../domain/settings.dart';
import '../../../state/repository_providers.dart';
import '../../../state/ui_providers.dart';
import '_setting_row.dart';

/// The General settings section: launch-at-login, tray/menu-bar
/// visibility, and appearance.
class GeneralSection extends ConsumerWidget {
  const GeneralSection({
    super.key,
    required this.palette,
    required this.chrome,
  });

  final FlowPalette palette;
  final PlatformChrome chrome;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = palette;
    final settings = ref.watch(settingsProvider).valueOrNull;
    final themeMode = ref.watch(themeModeProvider);

    void patch(SettingsPatch patch) =>
        ref.read(daemonRepositoryProvider).updateSettings(patch);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'General',
          style: FlowType.body(
            c.text1,
            weight: FontWeight.w700,
          ).copyWith(fontSize: 15),
        ),
        const SizedBox(height: 8),
        SettingRow(
          palette: c,
          label: 'Launch at login',
          description: 'Start quietly when this computer boots.',
          trailing: FlowToggle(
            value: settings?.launchAtLogin ?? true,
            activeColor: c.accent,
            trackColor: c.mat2,
            onChanged: (v) => patch(SettingsPatch(launchAtLogin: v)),
          ),
        ),
        SettingRow(
          palette: c,
          label: chrome.controls == ChromeControls.win
              ? 'Show in system tray'
              : 'Show in menu bar',
          description: 'Keep the icon visible.',
          trailing: FlowToggle(
            value: settings?.showTrayIcon ?? true,
            activeColor: c.accent,
            trackColor: c.mat2,
            onChanged: (v) => patch(SettingsPatch(showTrayIcon: v)),
          ),
        ),
        SettingRow(
          palette: c,
          label: 'Appearance',
          description: 'Match the system or pick a theme.',
          trailing: FlowSegmentedControl<ThemeMode>(
            segments: const [
              FlowSegment(value: ThemeMode.dark, label: 'Dark'),
              FlowSegment(value: ThemeMode.light, label: 'Light'),
            ],
            selected: themeMode,
            onChanged: (mode) =>
                ref.read(themeModeProvider.notifier).state = mode,
            background: c.mat1,
            selectedBackground: c.accent,
            selectedForeground: c.accentText,
            unselectedForeground: c.text2,
          ),
        ),
      ],
    );
  }
}
