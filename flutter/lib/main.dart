import 'package:flutter/material.dart';

import 'devices/devices_screen.dart';

void main() {
  runApp(const FlowApp());
}

/// Root widget for the Flow control-plane UI.
///
/// Per vision.md §8-§9, this UI only configures, pairs with, and observes
/// the Flow daemon over local IPC — the daemon keeps working even if this
/// app is closed.
class FlowApp extends StatelessWidget {
  const FlowApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Flow',
      theme: ThemeData(useMaterial3: true),
      home: const DevicesScreen(),
    );
  }
}
