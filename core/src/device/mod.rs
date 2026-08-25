//! Device identity and connection state (vision.md §13, Device State).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Opaque identifier for a paired device. Serializes transparently as its
/// inner string (matches `data-model.md`'s `Device.id: String`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub String);

/// Operating system a paired device is running, per `data-model.md`'s
/// `Device.os` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostOs {
    Macos,
    Windows,
    Linux,
}

/// Lifecycle state of a device as seen by this daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    Pairing,
    Connected,
    Active,
    Inactive,
    Disconnected,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Device {
    pub id: DeviceId,
    pub name: String,
    pub os: HostOs,
    pub state: DeviceState,
    pub last_seen: DateTime<Utc>,
}
