//! Device identity and connection state (vision.md §13, Device State).

/// Opaque identifier for a paired device.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceId(pub String);

/// Lifecycle state of a device as seen by this daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Pairing,
    Connected,
    Active,
    Inactive,
    Disconnected,
    Error,
}

#[derive(Debug, Clone)]
pub struct Device {
    pub id: DeviceId,
    pub name: String,
    pub state: DeviceState,
}
