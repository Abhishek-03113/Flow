//! Keyboard emulation: what the platform adapters do, driven by the real
//! per-OS keymap data (`config/keymaps/*.toml`).
//!
//! A physical key press on a device of OS `X` is a *native key code* of
//! `X`. The capture adapter turns it into a wire name with `X`'s keymap
//! (`name_for_code`); the injector on the receiving side turns a wire name
//! back into the receiver's native code (`code_for_name`). Routing both
//! directions through the embedded keymaps means every OS pair exercises
//! the real key data and the cross-OS translation.

use flow_core::device::HostOs;
use flow_core::keymap::Keymap;
use flow_core::protocol::Modifier;

pub struct OsKeyboard {
    os: HostOs,
    keymap: Keymap,
}

impl OsKeyboard {
    pub fn new(os: HostOs) -> Self {
        Self {
            os,
            keymap: Keymap::embedded(os),
        }
    }

    pub fn os(&self) -> HostOs {
        self.os
    }

    /// The native code of the key known on the wire as `name`, as a user
    /// would press it on this OS's keyboard. An error means the OS has no
    /// such key.
    pub fn native_code(&self, name: &str) -> Result<u32, String> {
        self.keymap
            .code_for_name(name)
            .ok_or_else(|| format!("{:?} has no key named {name} in its keymap", self.os))
    }

    /// What the capture adapter reports for native `code`: the keymap
    /// name, or the raw code when the keymap doesn't know it.
    pub fn captured_name(&self, code: u32) -> String {
        self.keymap
            .name_for_code(code)
            .map_or_else(|| format!("0x{code:02X}"), str::to_owned)
    }

    /// The code the injector would send for wire name `name`.
    pub fn injected_code(&self, name: &str) -> Option<u32> {
        self.keymap.code_for_name(name)
    }
}

/// The modifier a modifier key (left or right) stands for.
pub fn modifier_of(key_name: &str) -> Option<Modifier> {
    match key_name {
        "SHIFT" | "RIGHT_SHIFT" => Some(Modifier::Shift),
        "CONTROL" | "RIGHT_CONTROL" => Some(Modifier::Ctrl),
        "OPTION" | "RIGHT_OPTION" => Some(Modifier::Alt),
        "COMMAND" | "RIGHT_COMMAND" => Some(Modifier::Meta),
        _ => None,
    }
}

/// The wire key name a switch-key binding token is pressed with.
pub fn chord_key(token: &str) -> String {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "^" => "CONTROL",
        "shift" => "SHIFT",
        "alt" | "option" | "opt" => "OPTION",
        "meta" | "cmd" | "command" | "win" | "windows" | "super" => "COMMAND",
        "scrolllock" => "SCROLLLOCK",
        "pause" => "PAUSE",
        other => return other.to_ascii_uppercase(),
    }
    .to_string()
}

/// The key a typed character is pressed with (letters, digits, space).
pub fn key_for_char(c: char) -> Option<String> {
    match c {
        'a'..='z' | 'A'..='Z' | '0'..='9' => Some(c.to_ascii_uppercase().to_string()),
        ' ' => Some("SPACE".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_key_has_a_different_native_code_per_os() {
        let win = OsKeyboard::new(HostOs::Windows);
        let mac = OsKeyboard::new(HostOs::Macos);
        assert_eq!(win.native_code("A"), Ok(0x41));
        assert_eq!(mac.native_code("A"), Ok(0x00));
    }

    #[test]
    fn unmapped_codes_are_reported_raw_like_the_adapters_do() {
        assert_eq!(
            OsKeyboard::new(HostOs::Windows).captured_name(0x1234),
            "0x1234"
        );
    }

    #[test]
    fn binding_tokens_map_to_pressable_keys() {
        assert_eq!(chord_key("Ctrl"), "CONTROL");
        assert_eq!(chord_key("ScrollLock"), "SCROLLLOCK");
        assert_eq!(chord_key("F13"), "F13");
        assert_eq!(chord_key("Space"), "SPACE");
    }
}
