//! In-memory device registry and active-device tracking (vision.md §13).

use std::collections::HashMap;

use crate::device::{Device, DeviceId};

#[derive(Debug, Default)]
pub struct AppState {
    devices: HashMap<DeviceId, Device>,
    active_device: Option<DeviceId>,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert_device(&mut self, device: Device) {
        self.devices.insert(device.id.clone(), device);
    }

    pub fn device(&self, id: &DeviceId) -> Option<&Device> {
        self.devices.get(id)
    }

    pub fn active_device(&self) -> Option<&Device> {
        self.active_device
            .as_ref()
            .and_then(|id| self.devices.get(id))
    }

    pub fn set_active(&mut self, id: DeviceId) {
        self.active_device = Some(id);
    }
}
