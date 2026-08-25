import 'package:flutter/material.dart';

import '../../../core/theme/tokens.dart';

/// A label + description on the left, an arbitrary control (toggle,
/// segmented control, button) on the right, with the hairline-bottom
/// border every settings row in the source shares. Shared by every
/// section in `app_window/sections/` rather than four copies of the same
/// row layout.
class SettingRow extends StatelessWidget {
  const SettingRow({
    super.key,
    required this.palette,
    required this.label,
    this.description,
    required this.trailing,
  });

  final FlowPalette palette;
  final String label;
  final String? description;
  final Widget trailing;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    return Container(
      padding: const EdgeInsets.symmetric(vertical: 13),
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: c.hairline)),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  label,
                  style: FlowType.body(c.text1, weight: FontWeight.w600),
                ),
                if (description != null) ...[
                  const SizedBox(height: 2),
                  Text(description!, style: FlowType.meta(c.text2)),
                ],
              ],
            ),
          ),
          const SizedBox(width: 14),
          trailing,
        ],
      ),
    );
  }
}
