//! [`MacosInputInjector`]: posts synthetic `CGEvent`s built from incoming
//! `InputEvent`s via `CGEventPost`.

use std::fmt;

use core_graphics::event::CGEventTapLocation;
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use flow_core::input::InputInjector;
use flow_core::protocol::{InputEvent, MouseEvent};

use super::inject_translate::{to_cg_event, HeldButtons};

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
    /// Mouse buttons currently held down, tracked across events so a
    /// `MouseEvent::Move` arriving mid-press is posted as a drag — see
    /// [`HeldButtons`]. Single-threaded: each `MacosInputInjector` runs
    /// on its own dedicated injector thread (`daemon` `main.rs`).
    held: HeldButtons,
}

impl MacosInputInjector {
    pub fn new() -> Result<Self, MacosInjectError> {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|()| MacosInjectError::SourceCreationFailed)?;
        Ok(Self {
            source,
            held: HeldButtons::default(),
        })
    }
}

impl InputInjector for MacosInputInjector {
    type Error = MacosInjectError;

    fn inject(&mut self, event: &InputEvent) -> Result<(), Self::Error> {
        // Keep the held-button set current *before* translating, so a
        // `Move` between a `ButtonDown` and its `ButtonUp` is posted as
        // the matching `*MouseDragged` event.
        match event {
            InputEvent::Mouse(MouseEvent::ButtonDown { button, .. }) => self.held.press(*button),
            InputEvent::Mouse(MouseEvent::ButtonUp { button, .. }) => self.held.release(*button),
            _ => {}
        }
        if let Some(cg_event) = to_cg_event(&self.source, event, self.held) {
            cg_event.post(CGEventTapLocation::HID);
        }
        Ok(())
    }
}
