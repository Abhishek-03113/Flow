//! [`MacosInputInjector`]: posts synthetic `CGEvent`s built from incoming
//! `InputEvent`s via `CGEventPost`.

use std::fmt;

use core_graphics::event::CGEventTapLocation;
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use flow_core::input::InputInjector;
use flow_core::protocol::InputEvent;

use super::inject_translate::to_cg_event;

#[derive(Debug)]
pub enum MacosInjectError {
    /// `CGEventSourceCreate` returned null.
    SourceCreationFailed,
}

impl fmt::Display for MacosInjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceCreationFailed => write!(f, "CGEventSourceCreate failed"),
        }
    }
}

impl std::error::Error for MacosInjectError {}

/// Injects input by posting it through the HID event system
/// (`CGEventPost`) as a synthetic event, indistinguishable to other
/// processes from real hardware input.
pub struct MacosInputInjector {
    source: CGEventSource,
}

impl MacosInputInjector {
    pub fn new() -> Result<Self, MacosInjectError> {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|()| MacosInjectError::SourceCreationFailed)?;
        Ok(Self { source })
    }
}

impl InputInjector for MacosInputInjector {
    type Error = MacosInjectError;

    fn inject(&mut self, event: &InputEvent) -> Result<(), Self::Error> {
        if let Some(cg_event) = to_cg_event(&self.source, event) {
            cg_event.post(CGEventTapLocation::HID);
        }
        Ok(())
    }
}
