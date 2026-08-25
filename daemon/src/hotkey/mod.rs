//! Detects the configured switch-key combination in the platform-neutral
//! capture event stream (`daemon/todos.json` F1), independent of which
//! OS adapter (`flow-platform`) produced the events.
//!
//! `vision.md` §12: "Switching is the defining interaction of the
//! product... Users should eventually be able to choose... Custom key
//! combinations."

use std::collections::HashSet;

use flow_core::protocol::{InputEvent, KeyboardEvent, Modifier};
use flow_core::switch_key::SwitchKeyBinding;

/// A stateful matcher for one [`SwitchKeyBinding`], fed the raw capture
/// stream one event at a time.
///
/// A binding's tokens are either modifier names (`"Ctrl"`, `"Shift"`,
/// `"Alt"`, `"Meta"`) or literal key names (`"ScrollLock"`, `"F13"`,
/// `"Space"`, ...). Modifiers are checked against the *triggering*
/// event's own `modifiers` list — already a live, per-side-unified
/// snapshot every platform adapter's translator maintains — while
/// literal keys are checked against `held`, which this matcher tracks
/// itself from `KeyDown`/`KeyUp` pairs, since the protocol has no other
/// concept of "is this specific key currently down". Key-name comparison
/// is case-insensitive: the token vocabulary (`data-model.md`'s
/// `SwitchKeyBinding` presets) and each platform's own capture-layer
/// naming (`platform/src/*/translate.rs`) were designed independently
/// and don't share a casing convention (e.g. Linux reports `"SCROLLLOCK"`
/// for the token `"ScrollLock"`).
pub struct SwitchKeyMatcher {
    binding: SwitchKeyBinding,
    held: HashSet<String>,
}

impl SwitchKeyMatcher {
    pub fn new(binding: SwitchKeyBinding) -> Self {
        Self {
            binding,
            held: HashSet::new(),
        }
    }

    /// Swaps in a new binding, e.g. after `set_switch_key`/`update_settings`
    /// changes `ServiceState.settings.switch_key` mid-run (`daemon/todos.json`
    /// F1's live-reconfiguration criterion; the daemon doesn't need
    /// restarting). Clears tracked key state, since a combination that
    /// spans the swap (a key held before the change, released after)
    /// isn't a coherent match against either binding.
    pub fn set_binding(&mut self, binding: SwitchKeyBinding) {
        self.binding = binding;
        self.held.clear();
    }

    /// Feeds one capture event. Returns `true` exactly when this event
    /// completes the binding — every token satisfied simultaneously, not
    /// merely at some point during overlapping presses. Non-keyboard
    /// events and key releases never trigger a match.
    pub fn feed(&mut self, event: &InputEvent) -> bool {
        let InputEvent::Keyboard(keyboard_event) = event else {
            return false;
        };
        match keyboard_event {
            KeyboardEvent::KeyDown { key, modifiers, .. } => {
                self.held.insert(canonicalize(key));
                self.matches(modifiers)
            }
            KeyboardEvent::KeyUp { key, .. } => {
                self.held.remove(&canonicalize(key));
                false
            }
        }
    }

    fn matches(&self, current_modifiers: &[Modifier]) -> bool {
        self.binding.keys.iter().all(|token| {
            if let Some(modifier) = modifier_for_token(token) {
                current_modifiers.contains(&modifier)
            } else {
                self.held.contains(&canonicalize(token))
            }
        })
    }
}

fn canonicalize(key: &str) -> String {
    key.to_ascii_uppercase()
}

fn modifier_for_token(token: &str) -> Option<Modifier> {
    match token {
        "Ctrl" => Some(Modifier::Ctrl),
        "Shift" => Some(Modifier::Shift),
        "Alt" => Some(Modifier::Alt),
        "Meta" => Some(Modifier::Meta),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow_core::switch_key::presets;

    fn key_down(key: &str, modifiers: Vec<Modifier>) -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: key.to_string(),
            modifiers,
            timestamp_ms: 0,
        })
    }

    fn key_up(key: &str) -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyUp {
            key: key.to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        })
    }

    #[test]
    fn scroll_lock_preset_matches_on_a_single_key_token() {
        let mut matcher = SwitchKeyMatcher::new(presets()[0].clone());
        // Capture-layer naming (e.g. Linux's evdev-derived key_name)
        // doesn't share the preset token's casing on purpose — matching
        // has to be case-insensitive.
        assert!(matcher.feed(&key_down("SCROLLLOCK", vec![])));
    }

    #[test]
    fn ctrl_shift_space_only_matches_once_all_three_are_held_simultaneously() {
        let mut matcher = SwitchKeyMatcher::new(presets()[3].clone());

        assert!(!matcher.feed(&key_down("LCONTROL", vec![Modifier::Ctrl])));
        assert!(!matcher.feed(&key_down("LSHIFT", vec![Modifier::Shift, Modifier::Ctrl])));
        assert!(matcher.feed(&key_down("SPACE", vec![Modifier::Shift, Modifier::Ctrl])));
    }

    #[test]
    fn releasing_a_modifier_before_the_final_key_prevents_a_match() {
        let mut matcher = SwitchKeyMatcher::new(presets()[3].clone());

        assert!(!matcher.feed(&key_down("LCONTROL", vec![Modifier::Ctrl])));
        assert!(!matcher.feed(&key_down("LSHIFT", vec![Modifier::Shift, Modifier::Ctrl])));
        // Shift released before Space is pressed: the Space KeyDown's own
        // modifiers list no longer includes it.
        let _ = matcher.feed(&key_up("LSHIFT"));
        assert!(!matcher.feed(&key_down("SPACE", vec![Modifier::Ctrl])));
    }

    #[test]
    fn a_key_release_never_triggers_a_match() {
        let mut matcher = SwitchKeyMatcher::new(presets()[0].clone());
        assert!(matcher.feed(&key_down("SCROLLLOCK", vec![])));
        assert!(!matcher.feed(&key_up("SCROLLLOCK")));
    }

    #[test]
    fn a_mouse_event_never_triggers_a_match() {
        use flow_core::protocol::{MouseButton, MouseEvent};

        let mut matcher = SwitchKeyMatcher::new(presets()[0].clone());
        let event = InputEvent::Mouse(MouseEvent::ButtonDown {
            button: MouseButton::Left,
            timestamp_ms: 0,
        });
        assert!(!matcher.feed(&event));
    }

    #[test]
    fn changing_the_binding_mid_run_is_picked_up_without_reconstructing_the_matcher() {
        let mut matcher = SwitchKeyMatcher::new(presets()[0].clone()); // Scroll Lock
        assert!(matcher.feed(&key_down("SCROLLLOCK", vec![])));

        matcher.set_binding(presets()[2].clone()); // F13
        assert!(!matcher.feed(&key_down("SCROLLLOCK", vec![])));
        assert!(matcher.feed(&key_down("F13", vec![])));
    }
}
