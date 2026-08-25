//! The shortcut that switches the active device (`vision.md` §12,
//! `data-model.md` "SwitchKeyBinding").
//!
//! Kept as its own module since both `settings` (track A5) and the future
//! hotkey detector (track F) depend on it independently.

/// A switch-key shortcut: a human-readable label plus the ordered,
/// platform-neutral key tokens that make it up (e.g. `"Ctrl"`, `"Alt"`,
/// `"Shift"`, `"Meta"`, `"ScrollLock"`, `"Pause"`, `"F13"`, single
/// characters, ...). Rendering a platform-correct glyph (e.g. `⌘` on macOS
/// for `"Meta"`) is a UI concern, not part of this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchKeyBinding {
    pub label: String,
    pub keys: Vec<String>,
}

impl SwitchKeyBinding {
    fn new(label: &str, keys: &[&str]) -> Self {
        Self {
            label: label.to_string(),
            keys: keys.iter().map(|k| k.to_string()).collect(),
        }
    }
}

/// The four built-in presets, in the order `data-model.md` lists them:
/// Scroll Lock, Pause, F13, Ctrl + Shift + Space.
///
/// `SwitchKeyBinding` holds a `Vec<String>`, which can't be built in a
/// `const` initializer (heap allocation isn't allowed in stable const
/// evaluation) — this is a function rather than a `pub const` array for
/// that reason.
pub fn presets() -> [SwitchKeyBinding; 4] {
    [
        SwitchKeyBinding::new("Scroll Lock", &["ScrollLock"]),
        SwitchKeyBinding::new("Pause", &["Pause"]),
        SwitchKeyBinding::new("F13", &["F13"]),
        SwitchKeyBinding::new("Ctrl + Shift + Space", &["Ctrl", "Shift", "Space"]),
    ]
}

/// The default binding (Scroll Lock), per `vision.md` §12.
pub fn default_binding() -> SwitchKeyBinding {
    SwitchKeyBinding::new("Scroll Lock", &["ScrollLock"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_tokens_match_the_contract() {
        let p = presets();
        assert_eq!(p[0].keys, vec!["ScrollLock"]);
        assert_eq!(p[1].keys, vec!["Pause"]);
        assert_eq!(p[2].keys, vec!["F13"]);
        assert_eq!(p[3].keys, vec!["Ctrl", "Shift", "Space"]);
    }

    #[test]
    fn default_binding_is_scroll_lock() {
        assert_eq!(default_binding(), presets()[0]);
    }
}
