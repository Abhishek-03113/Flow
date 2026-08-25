//! Windows input adapter. Implementation pending: capture will use a
//! low-level keyboard/mouse hook (`SetWindowsHookEx`) and injection will
//! use `SendInput`.

use flow_core::input::{InputCapture, InputInjector};
use flow_core::protocol::InputEvent;

#[derive(Debug, Default)]
pub struct WindowsInputCapture;

impl InputCapture for WindowsInputCapture {
    type Error = std::io::Error;

    fn start(&mut self) -> Result<(), Self::Error> {
        unimplemented!("Windows input capture is not yet implemented")
    }

    fn stop(&mut self) -> Result<(), Self::Error> {
        unimplemented!("Windows input capture is not yet implemented")
    }
}

#[derive(Debug, Default)]
pub struct WindowsInputInjector;

impl InputInjector for WindowsInputInjector {
    type Error = std::io::Error;

    fn inject(&mut self, _event: &InputEvent) -> Result<(), Self::Error> {
        unimplemented!("Windows input injection is not yet implemented")
    }
}
