//! Device identity and connection state (vision.md §13, Device State).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::control::Role;

/// "This device": the machine the engine itself runs on. Never
/// removable, never a pairing candidate.
pub const LOCAL_DEVICE_ID: &str = "d1";

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

/// A screen's resolution in pixels (points on macOS), as reported by the
/// machine that owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScreenSize {
    pub width: u32,
    pub height: u32,
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Device {
    pub id: DeviceId,
    pub name: String,
    pub os: HostOs,
    pub state: DeviceState,
    pub last_seen: DateTime<Utc>,
    /// The peer's role, known once it has said hello on a live link.
    #[serde(default)]
    pub role: Option<Role>,
    /// The peer's screen, known once it has said hello on a live link.
    #[serde(default)]
    pub screen: Option<ScreenSize>,
    /// Why the last link attempt failed, e.g. `role_conflict`.
    #[serde(default)]
    pub link_error: Option<String>,
}

impl Serialize for Device {
    /// Adds `last_seen_ms` (epoch milliseconds) beside `last_seen`, derived
    /// so the two can never disagree.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut out = serializer.serialize_struct("Device", 10)?;
        out.serialize_field("id", &self.id)?;
        out.serialize_field("name", &self.name)?;
        out.serialize_field("os", &self.os)?;
        out.serialize_field("state", &self.state)?;
        out.serialize_field("last_seen", &self.last_seen)?;
        out.serialize_field("last_seen_ms", &self.last_seen.timestamp_millis())?;
        out.serialize_field("role", &self.role)?;
        out.serialize_field("screen", &self.screen)?;
        out.serialize_field("link_error", &self.link_error)?;
        out.serialize_field("is_local", &(self.id.0 == LOCAL_DEVICE_ID))?;
        out.end()
    }
}
