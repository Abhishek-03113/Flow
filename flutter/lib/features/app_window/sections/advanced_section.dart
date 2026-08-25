import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/theme/tokens.dart';
import '../../../core/widgets/primitives.dart';
import '../../../domain/settings.dart';
import '../../../state/repository_providers.dart';
import '../../../state/ui_providers.dart';
import '_setting_row.dart';

/// The Advanced settings section: diagnostics (static copy — not
/// daemon-sourced yet, see `docs/contracts/README.md`'s note on what's
/// out of scope for contract 0.1.0), debug logging, the permission
/// status (same provider/command the onboarding permission step uses),
/// and the destructive reset action.
class AdvancedSection extends ConsumerWidget {
  const AdvancedSection({super.key, required this.palette});

  final FlowPalette palette;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = palette;
    final settings = ref.watch(settingsProvider).valueOrNull;
    final permission = ref.watch(permissionProvider).valueOrNull;
    final granted = permission?.granted ?? false;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Advanced',
          style: FlowType.body(
            c.text1,
            weight: FontWeight.w700,
          ).copyWith(fontSize: 15),
        ),
        const SizedBox(height: 8),
        // Static copy — the daemon doesn't report real switch-time/drop
        // metrics yet (docs/contracts/README.md, "out of scope for 0.1.0").
        SettingRow(
          palette: c,
          label: 'Diagnostics',
          description: 'Average switch time 8 ms · no dropped input today',
          trailing: const SizedBox.shrink(),
        ),
        SettingRow(
          palette: c,
          label: 'Detailed logging',
          description: 'Keep a log for support requests.',
          trailing: FlowToggle(
            value: settings?.debugLogging ?? false,
            activeColor: c.accent,
            trackColor: c.mat2,
            onChanged: (v) => ref
                .read(daemonRepositoryProvider)
                .updateSettings(SettingsPatch(debugLogging: v)),
          ),
        ),
        SettingRow(
          palette: c,
          label: permission?.name ?? 'Permission',
          description: granted ? 'Granted' : 'Not granted yet',
          trailing: FlowButton(
            label: granted ? 'Granted' : 'Allow',
            kind: FlowButtonKind.primary,
            background: granted ? c.mat2 : c.accent,
            foreground: granted ? c.text2 : c.accentText,
            onPressed: granted
                ? null
                : () => ref.read(daemonRepositoryProvider).requestPermission(),
          ),
        ),
        const SizedBox(height: 18),
        FlowButton(
          label: 'Reset all settings',
          kind: FlowButtonKind.danger,
          background: c.dangerSoft,
          foreground: c.danger,
          onPressed: () async {
            await ref.read(daemonRepositoryProvider).resetSettings();
            ref.read(toastProvider.notifier).show('Settings reset');
          },
        ),
      ],
    );
  }
}
