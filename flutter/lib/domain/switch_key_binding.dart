/// The shortcut that switches the active device
/// (`docs/product/vision.md` §12).
///
/// [keys] tokens are platform-neutral strings ("Ctrl", "Alt", "Shift",
/// "Meta", "ScrollLock", "Pause", "F13", single characters, ...).
/// Rendering a platform-correct glyph (e.g. "⌘" on macOS for "Meta") is a
/// UI concern, not part of this type.
class SwitchKeyBinding {
  const SwitchKeyBinding({required this.label, required this.keys});

  final String label;
  final List<String> keys;

  /// The four built-in presets, in display order
  /// (`docs/contracts/data-model.md`).
  static const presets = <SwitchKeyBinding>[
    SwitchKeyBinding(label: 'Scroll Lock', keys: ['ScrollLock']),
    SwitchKeyBinding(label: 'Pause', keys: ['Pause']),
    SwitchKeyBinding(label: 'F13', keys: ['F13']),
    SwitchKeyBinding(label: 'Ctrl + Shift + Space', keys: ['Ctrl', 'Shift', 'Space']),
  ];

  static SwitchKeyBinding get defaultBinding => presets.first;

  @override
  bool operator ==(Object other) {
    if (other is! SwitchKeyBinding || other.label != label || other.keys.length != keys.length) {
      return false;
    }
    for (var i = 0; i < keys.length; i++) {
      if (other.keys[i] != keys[i]) return false;
    }
    return true;
  }

  @override
  int get hashCode => Object.hash(label, Object.hashAll(keys));

  @override
  String toString() => 'SwitchKeyBinding($label)';
}
