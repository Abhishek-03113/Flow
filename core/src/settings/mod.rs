//! Persisted user preferences (`data-model.md` "FlowSettings").

use serde::{Deserialize, Serialize};

use crate::control::Role;
use crate::switch_key::{default_binding, SwitchKeyBinding};

/// How fast the pointer moves relative to raw input deltas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerSensitivity {
    Low,
    Normal,
    High,
}

/// The full set of user-configurable settings, mirroring
/// `data-model.md`'s `FlowSettings` class field-for-field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FlowSettings {
    pub launch_at_login: bool,
    pub show_tray_icon: bool,
    pub auto_reconnect: bool,
    pub auto_connect_paired_devices: bool,
    pub share_keyboard: bool,
    pub share_mouse: bool,
    pub debug_logging: bool,
    pub pointer_sensitivity: PointerSensitivity,
    pub switch_key: SwitchKeyBinding,
    pub role: Role,
    /// Jumps straight back to the local screen; `None` = only the switch
    /// key cycles back.
    pub return_home_key: Option<SwitchKeyBinding>,
    /// Switch screens by moving the pointer through a screen edge.
    pub edge_switching: bool,
    /// How long the pointer must rest against an edge before it switches.
    pub edge_switch_delay_ms: u32,
    /// Where user keymap/remap files live. Read-only: set by the engine,
    /// never persisted or patched.
    pub keymap_overrides_path: String,
}

impl FlowSettings {
    /// Defaults per `data-model.md`'s JSON example: every bool `true`
    /// except `debug_logging`, `pointer_sensitivity` normal, switch key
    /// Scroll Lock.
    pub fn defaults() -> Self {
        Self {
            launch_at_login: false,
            show_tray_icon: true,
            auto_reconnect: true,
            auto_connect_paired_devices: true,
            share_keyboard: true,
            share_mouse: true,
            debug_logging: false,
            pointer_sensitivity: PointerSensitivity::Normal,
            switch_key: default_binding(),
            role: Role::Controller,
            return_home_key: None,
            edge_switching: true,
            edge_switch_delay_ms: 0,
            keymap_overrides_path: String::new(),
        }
    }

    /// Merge a partial patch in, field by field, only where `Some`.
    pub fn apply_patch(&mut self, patch: SettingsPatch) {
        let SettingsPatch {
            launch_at_login,
            show_tray_icon,
            auto_reconnect,
            auto_connect_paired_devices,
            share_keyboard,
            share_mouse,
            debug_logging,
            pointer_sensitivity,
            switch_key,
            role,
            return_home_key,
            edge_switching,
            edge_switch_delay_ms,
        } = patch;

        if let Some(v) = launch_at_login {
            self.launch_at_login = v;
        }
        if let Some(v) = show_tray_icon {
            self.show_tray_icon = v;
        }
        if let Some(v) = auto_reconnect {
            self.auto_reconnect = v;
        }
        if let Some(v) = auto_connect_paired_devices {
            self.auto_connect_paired_devices = v;
        }
        if let Some(v) = share_keyboard {
            self.share_keyboard = v;
        }
        if let Some(v) = share_mouse {
            self.share_mouse = v;
        }
        if let Some(v) = debug_logging {
            self.debug_logging = v;
        }
        if let Some(v) = pointer_sensitivity {
            self.pointer_sensitivity = v;
        }
        if let Some(v) = switch_key {
            self.switch_key = v;
        }
        if let Some(v) = role {
            self.role = v;
        }
        if let Some(v) = return_home_key {
            self.return_home_key = v;
        }
        if let Some(v) = edge_switching {
            self.edge_switching = v;
        }
        if let Some(v) = edge_switch_delay_ms {
            self.edge_switch_delay_ms = v;
        }
    }
}

/// A partial `FlowSettings` update — every field optional, matching the
/// The all-optional partial. Only the keys being
/// changed need to be `Some`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_at_login: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_tray_icon: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_reconnect: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_connect_paired_devices: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share_keyboard: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share_mouse: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug_logging: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer_sensitivity: Option<PointerSensitivity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub switch_key: Option<SwitchKeyBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    /// `Some(None)` (JSON `null`) clears the binding; absent leaves it.
    #[serde(
        default,
        deserialize_with = "present_or_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub return_home_key: Option<Option<SwitchKeyBinding>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_switching: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_switch_delay_ms: Option<u32>,
}

/// Distinguishes an absent field (`None`) from an explicit `null`
/// (`Some(None)`), which `Option<Option<T>>` alone can't.
fn present_or_null<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_contract_example() {
        let d = FlowSettings::defaults();
        assert!(!d.launch_at_login, "launching at login needs consent");
        assert!(d.show_tray_icon);
        assert!(d.auto_reconnect);
        assert!(d.auto_connect_paired_devices);
        assert!(d.share_keyboard);
        assert!(d.share_mouse);
        assert!(!d.debug_logging);
        assert_eq!(d.pointer_sensitivity, PointerSensitivity::Normal);
        assert_eq!(d.switch_key, default_binding());
        assert_eq!(d.role, Role::Controller);
        assert_eq!(d.return_home_key, None);
        assert!(d.edge_switching);
        assert_eq!(d.edge_switch_delay_ms, 0);
    }

    #[test]
    fn a_null_return_home_key_clears_it_and_an_absent_one_leaves_it() {
        let mut settings = FlowSettings::defaults();
        settings.return_home_key = Some(default_binding());

        settings.apply_patch(serde_json::from_str("{}").unwrap());
        assert!(settings.return_home_key.is_some());

        settings.apply_patch(serde_json::from_str(r#"{"return_home_key": null}"#).unwrap());
        assert_eq!(settings.return_home_key, None);
    }

    #[test]
    fn apply_patch_changes_only_the_given_field() {
        let mut settings = FlowSettings::defaults();
        let patch = SettingsPatch {
            share_mouse: Some(false),
            ..Default::default()
        };

        settings.apply_patch(patch);

        assert!(!settings.share_mouse);
        assert!(settings.share_keyboard);
        assert!(!settings.launch_at_login);
        assert_eq!(settings.pointer_sensitivity, PointerSensitivity::Normal);
    }
}
