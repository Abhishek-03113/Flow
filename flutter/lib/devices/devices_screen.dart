import 'package:flutter/material.dart';

/// Shows paired devices and which one is currently active
/// (vision.md §13, Device State).
class DevicesScreen extends StatelessWidget {
  const DevicesScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return const Scaffold(
      body: Center(child: Text('Flow')),
    );
  }
}
