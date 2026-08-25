//! [`LinuxInputInjector`]: replays `InputEvent`s into a virtual evdev
//! device created via uinput.

use std::io;

use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, KeyCode, RelativeAxisCode};
use flow_core::input::InputInjector;
use flow_core::protocol::InputEvent;

use super::inject_translate::to_uinput_events;

/// `input-event-codes.h`'s `KEY_MAX`: the highest code the `EV_KEY` event
/// type carries, spanning both standard keyboard keys and the `BTN_*`
/// mouse-button codes that share it. Declared broadly on purpose —
/// injected events can carry any evdev key name
/// (`translate::key_name`'s output), not just letters, so the virtual
/// device needs to support the full range up front (uinput requires
/// declaring supported keys before the device is created).
const MAX_KEY_CODE: u16 = 0x2ff;

/// Injects input by replaying it through a Flow-owned virtual device
/// (`Device::name()` reports "Flow Virtual Input"), rather than a real
/// keyboard/mouse.
pub struct LinuxInputInjector {
    device: VirtualDevice,
}

impl LinuxInputInjector {
    pub fn new() -> io::Result<Self> {
        let keys: AttributeSet<KeyCode> = (0..=MAX_KEY_CODE).map(KeyCode).collect();
        let mut relative_axes = AttributeSet::<RelativeAxisCode>::new();
        relative_axes.insert(RelativeAxisCode::REL_X);
        relative_axes.insert(RelativeAxisCode::REL_Y);
        relative_axes.insert(RelativeAxisCode::REL_WHEEL);
        relative_axes.insert(RelativeAxisCode::REL_HWHEEL);

        let device = VirtualDevice::builder()?
            .name("Flow Virtual Input")
            .with_keys(&keys)?
            .with_relative_axes(&relative_axes)?
            .build()?;
        Ok(Self { device })
    }
}

impl InputInjector for LinuxInputInjector {
    type Error = io::Error;

    fn inject(&mut self, event: &InputEvent) -> Result<(), Self::Error> {
        match to_uinput_events(event) {
            Some(events) => self.device.emit(&events),
            None => Ok(()),
        }
    }
}
