//! Linux input adapter. Implementation pending: capture and injection will
//! bind to evdev/uinput (or the desktop environment's equivalent).

use flow_core::input::{InputCapture, InputInjector};
use flow_core::protocol::InputEvent;

#[derive(Debug, Default)]
pub struct LinuxInputCapture;

impl InputCapture for LinuxInputCapture {
    type Error = std::io::Error;

    fn start(&mut self) -> Result<(), Self::Error> {
        unimplemented!("Linux input capture is not yet implemented")
    }

    fn stop(&mut self) -> Result<(), Self::Error> {
        unimplemented!("Linux input capture is not yet implemented")
    }
}

#[derive(Debug, Default)]
pub struct LinuxInputInjector;

impl InputInjector for LinuxInputInjector {
    type Error = std::io::Error;

    fn inject(&mut self, _event: &InputEvent) -> Result<(), Self::Error> {
        unimplemented!("Linux input injection is not yet implemented")
    }
}
