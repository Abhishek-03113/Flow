//! Capture/injection traits that isolate OS-specific input handling.
//!
//! The `flow-platform` crate implements these per operating system; the
//! daemon depends only on the traits, never on a concrete adapter.

use crate::protocol::InputEvent;

/// What to do with a physical event that was just captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Let this machine's own applications see it.
    Pass,
    /// Hide it from this machine's own applications.
    Swallow,
}

/// Receives captured input on the OS capture thread and decides, before
/// the OS delivers it, whether local applications get to see it.
///
/// The decision has to be synchronous: an asynchronous consumer would leave
/// a window after a switch in which events are still delivered locally
/// although they are also forwarded (or the other way round).
pub trait CaptureSink: Send + Sync {
    /// Called for every translated physical event, in order.
    fn on_event(&self, event: &InputEvent) -> Disposition;

    /// Whether physical input is currently hidden from local applications.
    /// Adapters use it for raw events with no [`InputEvent`] translation
    /// and, where hiding is per device rather than per event (Linux
    /// `EVIOCGRAB`), to know when to grab.
    fn suppressing(&self) -> bool;
}

pub trait InputCapture {
    type Error;

    fn start(&mut self) -> Result<(), Self::Error>;
    fn stop(&mut self) -> Result<(), Self::Error>;
}

pub trait InputInjector {
    type Error;

    fn inject(&mut self, event: &InputEvent) -> Result<(), Self::Error>;

    /// Moves the pointer to an absolute position. Adapters that can't
    /// (a relative-only virtual device) keep the default no-op.
    fn warp_cursor(&mut self, _x: i32, _y: i32) -> Result<(), Self::Error> {
        Ok(())
    }
}
