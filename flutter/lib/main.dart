import 'package:flutter/material.dart';

void main() {
  runApp(const _ScaffoldPlaceholderApp());
}

/// Placeholder entry point.
///
/// Real app wiring (ProviderScope, FlowTheme, navigation between the tray
/// popover / onboarding / app window) lands in `lib/app.dart` as part of
/// the app-shell track in `todos.json` (task S1) — this only keeps the
/// project runnable while the foundation and design-system tracks land.
class _ScaffoldPlaceholderApp extends StatelessWidget {
  const _ScaffoldPlaceholderApp();

  @override
  Widget build(BuildContext context) {
    return const MaterialApp(
      title: 'Flow',
      home: Scaffold(
        body: Center(child: Text('Flow UI — under construction')),
      ),
    );
  }
}
