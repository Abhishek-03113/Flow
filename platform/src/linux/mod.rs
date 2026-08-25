//! Linux input adapter, bound to evdev for capture (`daemon/todos.json`
//! E1). Injection (E2) still binds to `unimplemented!()` pending uinput
//! support.

mod capture;
mod discovery;
mod translate;

pub use capture::LinuxInputCapture;

use flow_core::input::InputInjector;
use flow_core::protocol::InputEvent;

#[derive(Debug, Default)]
pub struct LinuxInputInjector;

impl InputInjector for LinuxInputInjector {
    type Error = std::io::Error;

    fn inject(&mut self, _event: &InputEvent) -> Result<(), Self::Error> {
        unimplemented!("Linux input injection is not yet implemented")
    }
}
