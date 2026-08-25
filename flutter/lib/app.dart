import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'core/theme/flow_theme.dart';
import 'features/harness/dev_harness.dart';
import 'state/ui_providers.dart';

/// App root. Docking a real tray/menu-bar icon into the OS (a
/// `system_tray`-style plugin) is deliberately out of scope here — YAGNI
/// until there's a real daemon on the other end of the IPC connection to
/// dock against (`todos.json` S1). Until then, [DevHarness] is how the
/// tray popover, onboarding flow, and app window are reached: it's a
/// development/demo surface, not a fourth shipped screen.
class FlowApp extends ConsumerWidget {
  const FlowApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final themeMode = ref.watch(themeModeProvider);
    return MaterialApp(
      title: 'Flow',
      debugShowCheckedModeBanner: false,
      themeMode: themeMode,
      theme: FlowTheme.light(),
      darkTheme: FlowTheme.dark(),
      home: const DevHarness(),
    );
  }
}
