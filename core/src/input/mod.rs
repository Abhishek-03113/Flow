//! Capture/injection traits that isolate OS-specific input handling
//! (vision.md §7, Flutter Does Not Remove OS Complexity).
//!
//! The `flow-platform` crate implements these per operating system; the
//! daemon's core loop depends only on the traits, never on a concrete
//! adapter.

use crate::protocol::InputEvent;

pub trait InputCapture {
    type Error;

    fn start(&mut self) -> Result<(), Self::Error>;
    fn stop(&mut self) -> Result<(), Self::Error>;
}

pub trait InputInjector {
    type Error;

    fn inject(&mut self, event: &InputEvent) -> Result<(), Self::Error>;
}
