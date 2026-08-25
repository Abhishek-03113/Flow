//! Serializes one instance of every contract type and asserts the JSON
//! matches `docs/contracts/data-model.md`'s examples verbatim (compared as
//! `serde_json::Value`, so field order never causes a false failure).

use chrono::{TimeZone, Utc};
use serde_json::{json, Value};

use flow_core::device::{Device, DeviceId, DeviceState, HostOs};
use flow_core::link::DaemonLinkState;
use flow_core::pairing::{PairingCandidate, PairingSession, PairingStage};
use flow_core::permission::PermissionStatus;
use flow_core::settings::{FlowSettings, PointerSensitivity, SettingsPatch};
use flow_core::switch_key::SwitchKeyBinding;

fn value_of<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap()
}

#[test]
fn device_matches_data_model_example() {
    let device = Device {
        id: DeviceId("d2".to_string()),
        name: "Work Laptop".to_string(),
        os: HostOs::Windows,
        state: DeviceState::Inactive,
        last_seen: Utc.with_ymd_and_hms(2026, 8, 25, 6, 58, 0).unwrap(),
    };

    assert_eq!(
        value_of(&device),
        json!({
            "id": "d2",
            "name": "Work Laptop",
            "os": "windows",
            "state": "inactive",
            "last_seen": "2026-08-25T06:58:00Z"
        })
    );
}

#[test]
fn pairing_session_matches_data_model_example() {
    let session = PairingSession {
        stage: PairingStage::Requesting,
        candidates: vec![PairingCandidate {
            id: "cand-1".to_string(),
            name: "Office Mac Mini".to_string(),
            os: HostOs::Macos,
        }],
        target_name: Some("Office Mac Mini".to_string()),
        error: None,
    };

    assert_eq!(
        value_of(&session),
        json!({
            "stage": "requesting",
            "candidates": [{ "id": "cand-1", "name": "Office Mac Mini", "os": "macos" }],
            "target_name": "Office Mac Mini",
            "error": null
        })
    );
}

#[test]
fn switch_key_binding_matches_data_model_example() {
    let binding = SwitchKeyBinding {
        label: "Ctrl + Shift + Space".to_string(),
        keys: vec!["Ctrl".to_string(), "Shift".to_string(), "Space".to_string()],
    };

    assert_eq!(
        value_of(&binding),
        json!({ "label": "Ctrl + Shift + Space", "keys": ["Ctrl", "Shift", "Space"] })
    );
}

#[test]
fn flow_settings_defaults_match_data_model_example() {
    assert_eq!(
        value_of(&FlowSettings::defaults()),
        json!({
            "launch_at_login": true,
            "show_tray_icon": true,
            "auto_reconnect": true,
            "auto_connect_paired_devices": true,
            "share_keyboard": true,
            "share_mouse": true,
            "debug_logging": false,
            "pointer_sensitivity": "normal",
            "switch_key": { "label": "Scroll Lock", "keys": ["ScrollLock"] }
        })
    );
}

#[test]
fn settings_patch_only_serializes_the_keys_that_are_set() {
    let patch = SettingsPatch {
        share_mouse: Some(false),
        ..Default::default()
    };

    assert_eq!(value_of(&patch), json!({ "share_mouse": false }));
}

#[test]
fn permission_status_matches_data_model_example() {
    let status = PermissionStatus {
        name: "Accessibility access".to_string(),
        granted: false,
    };

    assert_eq!(
        value_of(&status),
        json!({ "name": "Accessibility access", "granted": false })
    );
}

#[test]
fn pointer_sensitivity_variants_are_snake_case() {
    assert_eq!(value_of(&PointerSensitivity::Low), json!("low"));
    assert_eq!(value_of(&PointerSensitivity::Normal), json!("normal"));
    assert_eq!(value_of(&PointerSensitivity::High), json!("high"));
}

#[test]
fn daemon_link_state_variants_are_snake_case() {
    assert_eq!(value_of(&DaemonLinkState::Connected), json!("connected"));
    assert_eq!(value_of(&DaemonLinkState::Connecting), json!("connecting"));
    assert_eq!(
        value_of(&DaemonLinkState::Reconnecting),
        json!("reconnecting")
    );
    assert_eq!(
        value_of(&DaemonLinkState::Disconnected),
        json!("disconnected")
    );
    assert_eq!(value_of(&DaemonLinkState::Error), json!("error"));
    assert_eq!(
        value_of(&DaemonLinkState::PermissionRequired),
        json!("permission_required")
    );
}

#[test]
fn device_state_variants_are_snake_case() {
    assert_eq!(value_of(&DeviceState::Pairing), json!("pairing"));
    assert_eq!(value_of(&DeviceState::Connected), json!("connected"));
    assert_eq!(value_of(&DeviceState::Active), json!("active"));
    assert_eq!(value_of(&DeviceState::Inactive), json!("inactive"));
    assert_eq!(value_of(&DeviceState::Disconnected), json!("disconnected"));
    assert_eq!(value_of(&DeviceState::Error), json!("error"));
}
