//! macOS input adapter, bound to a `CGEventTap` for capture
//! (`daemon/todos.json` E4). Injection (E5) still binds to
//! `unimplemented!()` pending `CGEventPost` support.

mod capture;
mod translate;

pub use capture::{MacosCaptureError, MacosInputCapture};

use flow_core::input::InputInjector;
use flow_core::protocol::InputEvent;

#[derive(Debug, Default)]
pub struct MacosInputInjector;

impl InputInjector for MacosInputInjector {
    type Error = std::io::Error;

    fn inject(&mut self, _event: &InputEvent) -> Result<(), Self::Error> {
        unimplemented!("macOS input injection is not yet implemented")
    }
}
