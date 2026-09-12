//! [`MacosInputInjector`]: posts synthetic `CGEvent`s built from incoming
//! `InputEvent`s via `CGEventPost`.

use std::fmt;

use core_graphics::event::{CGEventTapLocation, EventField};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use flow_core::input::InputInjector;
use flow_core::protocol::{InputEvent, KeyboardEvent, MouseEvent};

use super::inject_translate::{to_cg_event, HeldButtons};
use super::FLOW_INJECTED_MARKER;

#[derive(Debug)]
pub enum MacosInjectError {
    /// `CGEventSourceCreate` returned null.
    SourceCreationFailed,
    /// `to_cg_event` had no `CGKeyCode` for this key name — a key from a
    /// peer that sent something outside the shared vocabulary
    /// (`flow_core::protocol::key_names`) and macOS' own hex fallback.
    /// Surfaced as an error rather than silently doing nothing, per
    /// `InputCapture::set_suppress_local`'s stated philosophy in
    /// `core/src/input/mod.rs`: a caller that believes a key was
    /// injected when it wasn't is worse than a loud failure.
    UnmappedKey(String),
}

impl fmt::Display for MacosInjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceCreationFailed => write!(f, "CGEventSourceCreate failed"),
            Self::UnmappedKey(key) => write!(f, "no CGKeyCode for key {key:?}"),
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
        let Some(cg_event) = to_cg_event(&self.source, event, self.held) else {
            return match event {
                InputEvent::Keyboard(
                    KeyboardEvent::KeyDown { key, .. } | KeyboardEvent::KeyUp { key, .. },
                ) => Err(MacosInjectError::UnmappedKey(key.clone())),
                InputEvent::Mouse(_) => Ok(()),
            };
        };
        // Mark this as Flow's own output so an active capture tap in
        // this same process (`super::capture`) recognizes it on the
        // rebound and neither forwards it to the peer nor gates it.
        cg_event.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, FLOW_INJECTED_MARKER);
        cg_event.post(CGEventTapLocation::HID);
        Ok(())
    }
}
