//! The shortcut that switches the active device (`vision.md` §12,
//! `data-model.md` "SwitchKeyBinding").
//!
//! Kept as its own module since both `settings` (track A5) and the future
//! hotkey detector (track F) depend on it independently.

use serde::{Deserialize, Serialize};

use crate::protocol::{key_names, Modifier};

/// A switch-key shortcut: a human-readable label plus the ordered,
/// platform-neutral key tokens that make it up (e.g. `"Ctrl"`, `"Alt"`,
/// `"Shift"`, `"Meta"`, `"ScrollLock"`, `"Pause"`, `"F13"`, single
/// characters, ...). Rendering a platform-correct glyph (e.g. `⌘` on macOS
/// for `"Meta"`) is a UI concern, not part of this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// Why a binding can't be used as a global shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingProblem {
    /// No keys at all.
    Empty,
    /// A key used for typing (a letter, a digit, `Space`, ...) with no Ctrl,
    /// Alt or Meta held. The shortcut would swallow that key every time
    /// it is typed. Shift alone isn't enough: Shift + A is a capital A.
    NeedsModifier,
}

impl SwitchKeyBinding {
    /// A binding is usable when it has keys, and either holds Ctrl, Alt or
    /// Meta or its only other keys are ones nobody types with (Scroll Lock,
    /// Pause, F1–F24).
    pub fn check(&self) -> Result<(), BindingProblem> {
        if self.keys.is_empty() {
            return Err(BindingProblem::Empty);
        }
        let command_modifier = self
            .keys
            .iter()
            .filter_map(|t| modifier_for_token(t))
            .any(|m| m != Modifier::Shift);
        let only_standalone_keys = self
            .keys
            .iter()
            .filter(|t| modifier_for_token(t).is_none())
            .all(|t| is_standalone_key(&key_for_token(t)));
        if command_modifier || only_standalone_keys {
            Ok(())
        } else {
            Err(BindingProblem::NeedsModifier)
        }
    }

    /// Whether two bindings press the same keys, whatever their order,
    /// labels or spelling (`Ctrl` / `Control`, `Esc` / `ESCAPE`).
    pub fn same_keys(&self, other: &SwitchKeyBinding) -> bool {
        let normalized = |b: &SwitchKeyBinding| {
            let mut keys: Vec<String> = b
                .keys
                .iter()
                .map(|t| match modifier_for_token(t) {
                    Some(m) => format!("{m:?}"),
                    None => key_for_token(t),
                })
                .collect();
            keys.sort();
            keys.dedup();
            keys
        };
        normalized(self) == normalized(other)
    }
}

/// The one place binding tokens are normalised: users (and the UI) write
/// `Cmd`, `Win`, `⌘`, `Super` or `Meta` for the same key.
pub fn modifier_for_token(token: &str) -> Option<Modifier> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "^" => Some(Modifier::Ctrl),
        "shift" | "⇧" => Some(Modifier::Shift),
        "alt" | "option" | "opt" | "⌥" => Some(Modifier::Alt),
        "meta" | "cmd" | "command" | "win" | "windows" | "super" | "⌘" => Some(Modifier::Meta),
        _ => None,
    }
}

/// Binding tokens are the UI's (`Esc`, `Up`, `CapsLock`), events carry wire
/// names (`ESCAPE`, `UP_ARROW`, `CAPS_LOCK`): compare both as wire names.
pub fn key_for_token(token: &str) -> String {
    key_names::canonical(token).unwrap_or_else(|| token.to_ascii_uppercase())
}

/// Keys that are safe on their own: Scroll Lock, Pause and F1–F24.
fn is_standalone_key(wire_name: &str) -> bool {
    wire_name == key_names::SCROLLLOCK
        || wire_name == key_names::PAUSE
        || wire_name
            .strip_prefix('F')
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (1..=24).contains(&n))
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

    fn binding(keys: &[&str]) -> SwitchKeyBinding {
        SwitchKeyBinding::new("x", keys)
    }

    #[test]
    fn presets_and_modified_chords_are_usable() {
        for p in presets() {
            assert_eq!(p.check(), Ok(()), "{p:?}");
        }
        for keys in [
            &["Ctrl", "Up"][..],
            &["Alt", "A"],
            &["Meta", "Shift", "K"],
            &["Shift", "F13"],
            &["F1"],
            &["F24"],
            &["SCROLLLOCK"],
        ] {
            assert_eq!(binding(keys).check(), Ok(()), "{keys:?}");
        }
    }

    #[test]
    fn typing_keys_need_ctrl_alt_or_meta() {
        for keys in [
            &["A"][..],
            &["Space"],
            &["Shift", "A"],
            &["1"],
            &["Esc"],
            &["F25"],
        ] {
            assert_eq!(
                binding(keys).check(),
                Err(BindingProblem::NeedsModifier),
                "{keys:?}"
            );
        }
        assert_eq!(binding(&[]).check(), Err(BindingProblem::Empty));
    }

    #[test]
    fn same_keys_ignores_order_and_spelling() {
        assert!(binding(&["Ctrl", "Shift", "Space"])
            .same_keys(&binding(&["Shift", "Control", "SPACE"])));
        assert!(binding(&["ScrollLock"]).same_keys(&binding(&["SCROLLLOCK"])));
        assert!(!binding(&["Ctrl", "Space"]).same_keys(&binding(&["Ctrl", "Shift", "Space"])));
        assert!(!binding(&["Pause"]).same_keys(&binding(&["F13"])));
    }
}
