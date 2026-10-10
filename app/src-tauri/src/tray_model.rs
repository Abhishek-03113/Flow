//! What the native tray menu says, as plain data. No Tauri types here:
//! the model follows the engine's events and renders to a list of entries
//! that `tray.rs` turns into OS menu items.

use flow_core::control::{ActiveScreen, ControlState, Role};
use flow_core::device::{Device, DeviceState, LOCAL_DEVICE_ID};
use flow_core::link::DaemonLinkState;
use flow_core::pairing::IncomingPairingRequest;
use flow_daemon::ipc::events::StateEvent;
use serde::de::DeserializeOwned;

pub const ID_STATUS: &str = "status";
pub const ID_HOME: &str = "home";
pub const ID_REQUEST: &str = "request";
pub const ID_OPEN: &str = "open";
pub const ID_QUIT: &str = "quit";
pub const SWITCH_PREFIX: &str = "switch:";

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Item {
        id: String,
        label: String,
        enabled: bool,
        /// `Some` renders a check mark item.
        checked: Option<bool>,
    },
    Separator,
}

fn item(id: &str, label: impl Into<String>, enabled: bool) -> Entry {
    Entry::Item {
        id: id.to_string(),
        label: label.into(),
        enabled,
        checked: None,
    }
}

#[derive(Debug, Default)]
pub struct TrayModel {
    devices: Vec<Device>,
    control: Option<ControlState>,
    link: Option<DaemonLinkState>,
    request: Option<IncomingPairingRequest>,
}

fn decode<T: DeserializeOwned>(event: &StateEvent) -> Option<T> {
    serde_json::from_value(event.payload.clone()).ok()
}

impl TrayModel {
    /// Folds one engine event in; true when the menu must be rebuilt.
    pub fn apply(&mut self, event: &StateEvent) -> bool {
        match event.name {
            "devices_changed" => decode(event).map(|v| self.devices = v).is_some(),
            "control_state_changed" => decode(event).map(|v| self.control = Some(v)).is_some(),
            "link_state_changed" => decode(event).map(|v| self.link = Some(v)).is_some(),
            "incoming_pairing_request_changed" => {
                self.request = decode(event).unwrap_or(None);
                true
            }
            _ => false,
        }
    }

    fn remotes(&self) -> impl Iterator<Item = &Device> {
        self.devices.iter().filter(|d| d.id.0 != LOCAL_DEVICE_ID)
    }

    fn is_controller(&self) -> bool {
        self.control
            .as_ref()
            .is_none_or(|c| c.role == Role::Controller)
    }

    fn active_remote(&self) -> Option<&Device> {
        let control = self.control.as_ref()?;
        match &control.active_screen {
            ActiveScreen::Remote(id) if control.role == Role::Controller => {
                self.devices.iter().find(|d| d.id == *id)
            }
            _ => None,
        }
    }

    /// The first line of the menu, matching the UI's status copy.
    pub fn status_line(&self) -> String {
        if self.link == Some(DaemonLinkState::PermissionRequired) {
            return "Flow needs permission".into();
        }
        if self.remotes().next().is_none() {
            return "No computers yet".into();
        }
        match self.link {
            Some(DaemonLinkState::Reconnecting) => return "Reconnecting\u{2026}".into(),
            Some(DaemonLinkState::Connecting) => return "Connecting\u{2026}".into(),
            Some(DaemonLinkState::Error) => return "Sharing is paused".into(),
            Some(DaemonLinkState::Disconnected) => return "Not connected".into(),
            _ => {}
        }
        if let Some(target) = self.active_remote() {
            return format!("Controlling {}", target.name);
        }
        match &self.control {
            Some(c) if c.role == Role::Controlled => match &c.controller_device_id {
                Some(id) if c.entered => {
                    let name = self
                        .devices
                        .iter()
                        .find(|d| d.id == *id)
                        .map_or("Another computer", |d| d.name.as_str());
                    format!("{name} is controlling this computer")
                }
                Some(_) => "Ready to be controlled".into(),
                None => "Waiting for a controller".into(),
            },
            _ => "Using this computer".into(),
        }
    }

    /// The whole menu, top to bottom.
    pub fn entries(&self) -> Vec<Entry> {
        let mut out = vec![item(ID_STATUS, self.status_line(), false)];
        if let Some(request) = &self.request {
            out.push(item(
                ID_REQUEST,
                format!(
                    "Review pairing request from {}\u{2026}",
                    request.device_name
                ),
                true,
            ));
        }
        let remotes: Vec<_> = self.remotes().collect();
        if !remotes.is_empty() {
            out.push(Entry::Separator);
            let controller = self.is_controller();
            let active_id = self.active_remote().map(|d| d.id.0.clone());
            for device in remotes {
                let reachable = matches!(
                    device.state,
                    DeviceState::Inactive | DeviceState::Connected | DeviceState::Active
                ) && device.link_error.is_none();
                let label = if reachable {
                    format!("Switch to {}", device.name)
                } else {
                    format!("{} (offline)", device.name)
                };
                out.push(Entry::Item {
                    id: format!("{SWITCH_PREFIX}{}", device.id.0),
                    label,
                    enabled: controller && reachable,
                    checked: Some(active_id.as_deref() == Some(device.id.0.as_str())),
                });
            }
            if self.active_remote().is_some() {
                out.push(item(ID_HOME, "Return to This Computer", true));
            }
        }
        out.push(Entry::Separator);
        out.push(item(ID_OPEN, "Open Flow", true));
        out.push(item(ID_QUIT, "Quit Flow", true));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(name: &'static str, payload: serde_json::Value) -> StateEvent {
        StateEvent { name, payload }
    }

    fn device(id: &str, name: &str, state: &str) -> serde_json::Value {
        json!({ "id": id, "name": name, "os": "windows", "state": state,
                "last_seen": "2026-01-01T00:00:00Z", "role": "controlled",
                "screen": null, "link_error": null })
    }

    fn model(devices: serde_json::Value, active: &str, link: &str) -> TrayModel {
        let mut m = TrayModel::default();
        m.apply(&event("devices_changed", devices));
        m.apply(&event(
            "control_state_changed",
            json!({ "role": "controller", "active_screen": active,
                    "controller_device_id": null, "entered": false }),
        ));
        m.apply(&event("link_state_changed", json!(link)));
        m
    }

    fn ids(m: &TrayModel) -> Vec<String> {
        m.entries()
            .into_iter()
            .filter_map(|e| match e {
                Entry::Item { id, .. } => Some(id),
                Entry::Separator => None,
            })
            .collect()
    }

    #[test]
    fn a_fresh_install_says_no_computers_and_offers_only_open_and_quit() {
        let m = model(
            json!([device("d1", "Me", "active")]),
            "local",
            "disconnected",
        );
        assert_eq!(m.status_line(), "No computers yet");
        assert_eq!(ids(&m), ["status", "open", "quit"]);
    }

    #[test]
    fn controlling_a_remote_lists_devices_and_return_home() {
        let m = model(
            json!([
                device("d1", "Me", "inactive"),
                device("pk:a", "Office PC", "active"),
                device("pk:b", "Studio Mac", "inactive")
            ]),
            "pk:a",
            "connected",
        );
        assert_eq!(m.status_line(), "Controlling Office PC");
        assert_eq!(
            ids(&m),
            [
                "status",
                "switch:pk:a",
                "switch:pk:b",
                "home",
                "open",
                "quit"
            ]
        );
        let entries = m.entries();
        assert!(
            matches!(&entries[2], Entry::Item { id, checked: Some(true), .. } if id == "switch:pk:a")
        );
        assert!(
            matches!(&entries[3], Entry::Item { id, checked: Some(false), enabled: true, .. } if id == "switch:pk:b")
        );
    }

    #[test]
    fn an_offline_computer_cannot_be_switched_to_and_reconnecting_shows_in_the_status() {
        let m = model(
            json!([
                device("d1", "Me", "active"),
                device("pk:a", "Office PC", "disconnected")
            ]),
            "local",
            "reconnecting",
        );
        assert_eq!(m.status_line(), "Reconnecting\u{2026}");
        let entries = m.entries();
        assert!(
            matches!(&entries[2], Entry::Item { enabled: false, label, .. } if label == "Office PC (offline)")
        );
        assert!(!ids(&m).contains(&"home".to_string()));
    }

    #[test]
    fn a_controlled_computer_cannot_switch_and_names_who_drives_it() {
        let mut m = TrayModel::default();
        m.apply(&event(
            "devices_changed",
            json!([
                device("d1", "Me", "active"),
                device("pk:a", "Desk PC", "inactive")
            ]),
        ));
        m.apply(&event(
            "control_state_changed",
            json!({ "role": "controlled", "active_screen": "local",
                    "controller_device_id": "pk:a", "entered": true }),
        ));
        m.apply(&event("link_state_changed", json!("connected")));
        assert_eq!(m.status_line(), "Desk PC is controlling this computer");
        assert!(matches!(
            &m.entries()[2],
            Entry::Item { enabled: false, .. }
        ));
    }

    #[test]
    fn an_incoming_pairing_request_adds_a_review_item() {
        let mut m = model(
            json!([device("d1", "Me", "active")]),
            "local",
            "disconnected",
        );
        m.apply(&event(
            "incoming_pairing_request_changed",
            json!({ "request_id": "r", "device_name": "Laptop", "device_os": "macos",
                    "fingerprint": "aa", "address": "" }),
        ));
        assert_eq!(ids(&m), ["status", "request", "open", "quit"]);
    }
}
