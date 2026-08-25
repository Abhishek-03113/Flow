//! Windows input adapter, bound to `WH_KEYBOARD_LL`/`WH_MOUSE_LL` hooks
//! for capture (`daemon/todos.json` E6). Injection (E7) still binds to
//! `unimplemented!()` pending `SendInput` support.

mod capture;
mod translate;

pub use capture::{WindowsCaptureError, WindowsInputCapture};

use flow_core::input::InputInjector;
use flow_core::protocol::InputEvent;

#[derive(Debug, Default)]
pub struct WindowsInputInjector;

impl InputInjector for WindowsInputInjector {
    type Error = std::io::Error;

    fn inject(&mut self, _event: &InputEvent) -> Result<(), Self::Error> {
        unimplemented!("Windows input injection is not yet implemented")
    }
}
