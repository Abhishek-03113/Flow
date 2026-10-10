//! The controller/controlled model (`docs/adr/0001-controller-controlled-input-model.md`).

pub mod layout;

use serde::{Deserialize, Serialize};

use crate::device::DeviceId;

/// What a device does in a pairing. Static per device, not a baton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Shares its physical keyboard and mouse; the only side that
    /// captures input and the only side that switches.
    Controller,
    /// Is driven by a controller; never captures input for forwarding.
    Controlled,
}

/// Which screen the controller's input currently drives. Serializes as
/// `"local"` or the remote device's id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ActiveScreen {
    Local,
    Remote(DeviceId),
}

impl ActiveScreen {
    pub fn remote(&self) -> Option<&DeviceId> {
        match self {
            ActiveScreen::Local => None,
            ActiveScreen::Remote(id) => Some(id),
        }
    }
}

const LOCAL: &str = "local";

impl Serialize for ActiveScreen {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ActiveScreen::Local => serializer.serialize_str(LOCAL),
            ActiveScreen::Remote(id) => serializer.serialize_str(&id.0),
        }
    }
}

impl<'de> Deserialize<'de> for ActiveScreen {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(if raw == LOCAL {
            ActiveScreen::Local
        } else {
            ActiveScreen::Remote(DeviceId(raw))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlState {
    pub role: Role,
    /// Controller: where its input goes. Controlled: always `local`.
    pub active_screen: ActiveScreen,
    /// Controlled only: the controller currently linked, if any.
    pub controller_device_id: Option<DeviceId>,
    /// Controlled only: true while the controller is driving this machine.
    pub entered: bool,
}

impl ControlState {
    pub fn idle(role: Role) -> Self {
        Self {
            role,
            active_screen: ActiveScreen::Local,
            controller_device_id: None,
            entered: false,
        }
    }
}

/// What the Advanced settings page shows about how the engine is doing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnostics {
    /// Time from a switch request to the target being told to take over;
    /// `null` until a switch happened.
    pub switch_latency_ms_p50: Option<f64>,
    pub switch_latency_ms_p95: Option<f64>,
    pub events_forwarded: u64,
    pub events_dropped: u64,
    pub uptime_s: u64,
}

impl Diagnostics {
    pub fn from_samples(
        mut latencies_ms: Vec<f64>,
        events_forwarded: u64,
        events_dropped: u64,
        uptime_s: u64,
    ) -> Self {
        latencies_ms.sort_by(f64::total_cmp);
        Self {
            switch_latency_ms_p50: percentile(&latencies_ms, 50),
            switch_latency_ms_p95: percentile(&latencies_ms, 95),
            events_forwarded,
            events_dropped,
            uptime_s,
        }
    }
}

/// Nearest-rank percentile of an ascending sample.
fn percentile(sorted: &[f64], pct: usize) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (pct * sorted.len()).div_ceil(100).max(1);
    Some(sorted[rank - 1])
}

#[cfg(test)]
mod tests {
    #[test]
    fn percentiles_use_the_nearest_rank() {
        let samples: Vec<f64> = (1..=100).map(f64::from).collect();
        let d = Diagnostics::from_samples(samples, 5, 1, 9);
        assert_eq!(d.switch_latency_ms_p50, Some(50.0));
        assert_eq!(d.switch_latency_ms_p95, Some(95.0));
        assert_eq!(
            (d.events_forwarded, d.events_dropped, d.uptime_s),
            (5, 1, 9)
        );
        assert_eq!(
            Diagnostics::from_samples(vec![], 0, 0, 0).switch_latency_ms_p50,
            None
        );
        assert_eq!(
            Diagnostics::from_samples(vec![7.0], 0, 0, 0).switch_latency_ms_p95,
            Some(7.0)
        );
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn active_screen_is_a_plain_string_on_the_wire() {
        assert_eq!(
            serde_json::to_value(ActiveScreen::Local).unwrap(),
            json!("local")
        );
        let remote = ActiveScreen::Remote(DeviceId("pk:ab".to_string()));
        assert_eq!(serde_json::to_value(&remote).unwrap(), json!("pk:ab"));
        assert_eq!(
            serde_json::from_value::<ActiveScreen>(json!("pk:ab")).unwrap(),
            remote
        );
        assert_eq!(
            serde_json::from_value::<ActiveScreen>(json!("local")).unwrap(),
            ActiveScreen::Local
        );
    }

    #[test]
    fn control_state_matches_the_contract_shape() {
        let state = ControlState::idle(Role::Controller);
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            json!({
                "role": "controller",
                "active_screen": "local",
                "controller_device_id": null,
                "entered": false
            })
        );
    }
}
