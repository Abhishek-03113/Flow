//! macOS input adapter. Implementation pending: capture will use a
//! CGEventTap and injection will post events via CGEventPost (or
//! equivalent).

use flow_core::input::{InputCapture, InputInjector};
use flow_core::protocol::InputEvent;

#[derive(Debug, Default)]
pub struct MacosInputCapture;

impl InputCapture for MacosInputCapture {
    type Error = std::io::Error;

    fn start(&mut self) -> Result<(), Self::Error> {
        unimplemented!("macOS input capture is not yet implemented")
    }

    fn stop(&mut self) -> Result<(), Self::Error> {
        unimplemented!("macOS input capture is not yet implemented")
    }
}

#[derive(Debug, Default)]
pub struct MacosInputInjector;

impl InputInjector for MacosInputInjector {
    type Error = std::io::Error;

    fn inject(&mut self, _event: &InputEvent) -> Result<(), Self::Error> {
        unimplemented!("macOS input injection is not yet implemented")
    }
}
