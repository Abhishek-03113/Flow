import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/theme/tokens.dart';
import '../../../core/widgets/primitives.dart';
import '../../../domain/device.dart';
import '../../../domain/settings.dart';
import '../../../domain/switch_key_binding.dart';
import '../../../state/repository_providers.dart';
import '../../../state/ui_providers.dart';
import '_setting_row.dart';

const _sensitivityLabels = {
  PointerSensitivity.low: 'Low',
  PointerSensitivity.normal: 'Normal',
  PointerSensitivity.high: 'High',
};

/// The Input settings section: switch-key display/recorder, presets,
/// keyboard/mouse sharing toggles, pointer sensitivity.
class InputSection extends ConsumerStatefulWidget {
  const InputSection({
    super.key,
    required this.palette,
    required this.platform,
  });

  final FlowPalette palette;
  final HostOs platform;

  @override
  ConsumerState<InputSection> createState() => _InputSectionState();
}

class _InputSectionState extends ConsumerState<InputSection> {
  final _focusNode = FocusNode();
  bool _recording = false;

  static final _bareModifiers = {
    LogicalKeyboardKey.control,
    LogicalKeyboardKey.controlLeft,
    LogicalKeyboardKey.controlRight,
    LogicalKeyboardKey.shift,
    LogicalKeyboardKey.shiftLeft,
    LogicalKeyboardKey.shiftRight,
    LogicalKeyboardKey.alt,
    LogicalKeyboardKey.altLeft,
    LogicalKeyboardKey.altRight,
    LogicalKeyboardKey.meta,
    LogicalKeyboardKey.metaLeft,
    LogicalKeyboardKey.metaRight,
  };

  @override
  void dispose() {
    _focusNode.dispose();
    super.dispose();
  }

  void _toggleRecording() {
    setState(() => _recording = !_recording);
    if (_recording) _focusNode.requestFocus();
  }

  KeyEventResult _handleKey(FocusNode node, KeyEvent event) {
    if (!_recording || event is! KeyDownEvent) return KeyEventResult.ignored;
    if (_bareModifiers.contains(event.logicalKey)) {
      return KeyEventResult.handled;
    }

    final hk = HardwareKeyboard.instance;
    final parts = <String>[
      if (hk.isControlPressed) 'Ctrl',
      if (hk.isAltPressed) 'Alt',
      if (hk.isShiftPressed) 'Shift',
      if (hk.isMetaPressed) (widget.platform == HostOs.macos ? '⌘' : 'Win'),
      _labelFor(event.logicalKey),
    ];

    ref
        .read(daemonRepositoryProvider)
        .setSwitchKey(SwitchKeyBinding(label: parts.join(' + '), keys: parts));
    ref.read(toastProvider.notifier).show('Switch key updated');
    setState(() => _recording = false);
    return KeyEventResult.handled;
  }

  String _labelFor(LogicalKeyboardKey key) {
    if (key == LogicalKeyboardKey.space) return 'Space';
    final label = key.keyLabel;
    return label.length == 1 ? label.toUpperCase() : label;
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.palette;
    final settings = ref.watch(settingsProvider).valueOrNull;
    final switchKey = settings?.switchKey ?? SwitchKeyBinding.defaultBinding;

    void patch(SettingsPatch patch) =>
        ref.read(daemonRepositoryProvider).updateSettings(patch);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Input',
          style: FlowType.body(
            c.text1,
            weight: FontWeight.w700,
          ).copyWith(fontSize: 15),
        ),
        const SizedBox(height: 8),
        Focus(
          focusNode: _focusNode,
          onKeyEvent: _handleKey,
          child: Container(
            width: double.infinity,
            margin: const EdgeInsets.only(bottom: 14),
            padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 18),
            decoration: BoxDecoration(
              color: c.mat1,
              borderRadius: BorderRadius.circular(14),
            ),
            child: Column(
              children: [
                Text('Switch key', style: FlowType.meta(c.text2)),
                const SizedBox(height: 4),
                _recording
                    ? BreathePulse(
                        child: Text(
                          'Press any key…',
                          style: TextStyle(
                            fontSize: 20,
                            fontWeight: FontWeight.w700,
                            color: c.accent,
                          ),
                        ),
                      )
                    : Text(
                        switchKey.label,
                        style: TextStyle(
                          fontSize: 20,
                          fontWeight: FontWeight.w700,
                          color: c.text1,
                        ),
                      ),
                const SizedBox(height: 2),
                FlowButton(
                  label: _recording ? 'Cancel' : 'Record shortcut',
                  kind: FlowButtonKind.ghost,
                  background: c.mat2,
                  foreground: c.text1,
                  onPressed: _toggleRecording,
                ),
              ],
            ),
          ),
        ),
        Wrap(
          spacing: 7,
          runSpacing: 7,
          children: [
            for (final preset in SwitchKeyBinding.presets)
              FlowChip(
                label: preset.label,
                selected: switchKey.label == preset.label,
                selectedColor: c.accent,
                selectedForeground: c.accentText,
                background: c.mat1,
                foreground: c.text2,
                onPressed: () =>
                    ref.read(daemonRepositoryProvider).setSwitchKey(preset),
              ),
          ],
        ),
        const SizedBox(height: 12),
        SettingRow(
          palette: c,
          label: 'Share keyboard',
          description: 'Send typing to the active computer.',
          trailing: FlowToggle(
            value: settings?.shareKeyboard ?? true,
            activeColor: c.accent,
            trackColor: c.mat2,
            onChanged: (v) => patch(SettingsPatch(shareKeyboard: v)),
          ),
        ),
        SettingRow(
          palette: c,
          label: 'Share mouse',
          description: 'Send pointer and scroll to the active computer.',
          trailing: FlowToggle(
            value: settings?.shareMouse ?? true,
            activeColor: c.accent,
            trackColor: c.mat2,
            onChanged: (v) => patch(SettingsPatch(shareMouse: v)),
          ),
        ),
        SettingRow(
          palette: c,
          label: 'Pointer sensitivity',
          trailing: FlowSegmentedControl<PointerSensitivity>(
            segments: [
              for (final entry in _sensitivityLabels.entries)
                FlowSegment(value: entry.key, label: entry.value),
            ],
            selected: settings?.pointerSensitivity ?? PointerSensitivity.normal,
            onChanged: (s) => patch(SettingsPatch(pointerSensitivity: s)),
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
