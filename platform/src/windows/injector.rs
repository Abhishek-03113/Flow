//! [`WindowsInputInjector`]: builds `INPUT` structs from incoming
//! `InputEvent`s and sends them via `SendInput`.

use std::fmt;
use std::mem::size_of;

use flow_core::input::InputInjector;
use flow_core::protocol::{InputEvent, KeyboardEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};

use super::inject_translate::to_input;

#[derive(Debug)]
pub enum WindowsInjectError {
    /// `SendInput` reported it queued fewer events than were sent —
    /// per Win32 docs, this means another thread's input was already
    /// blocking the input stream (e.g. a UIPI-protected foreground
    /// window), not a transient failure worth retrying blindly.
    SendInputBlocked,
    /// `to_input` had no `VIRTUAL_KEY` for this key name — a key from a
    /// peer that sent something outside the shared vocabulary
    /// (`flow_core::protocol::key_names`) and Windows' own hex fallback.
    /// Surfaced as an error (`daemon/src/pipeline/mod.rs`'s
    /// `receive_and_inject` logs it) rather than silently doing nothing,
    /// per `InputCapture::set_suppress_local`'s stated philosophy in
    /// `core/src/input/mod.rs`: a caller that believes a key was
    /// injected when it wasn't is worse than a loud failure.
    UnmappedKey(String),
}

impl fmt::Display for WindowsInjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SendInputBlocked => {
                write!(f, "SendInput did not queue all events (input blocked)")
            }
            Self::UnmappedKey(key) => write!(f, "no VIRTUAL_KEY for key {key:?}"),
        }
    }
}

impl std::error::Error for WindowsInjectError {}

/// Injects input by queuing synthetic `INPUT` events into the same
/// stream real hardware feeds, via `SendInput`.
#[derive(Debug, Default)]
pub struct WindowsInputInjector;

impl InputInjector for WindowsInputInjector {
    type Error = WindowsInjectError;

    fn inject(&mut self, event: &InputEvent) -> Result<(), Self::Error> {
        let Some(inputs) = to_input(event) else {
            return match event {
                InputEvent::Keyboard(
                    KeyboardEvent::KeyDown { key, .. } | KeyboardEvent::KeyUp { key, .. },
                ) => Err(WindowsInjectError::UnmappedKey(key.clone())),
                // A mouse event's `to_input` never returns `None` today,
                // but if it ever grows a case that does, silently
                // dropping it (as before this change) is preferable to
                // guessing at an error for a case that isn't understood
                // yet.
                InputEvent::Mouse(_) => Ok(()),
            };
        };
        // SAFETY: `inputs` is a slice of well-formed INPUT values built
        // by `to_input`; SendInput's usual FFI contract.
        let queued = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
        if (queued as usize) < inputs.len() {
            return Err(WindowsInjectError::SendInputBlocked);
        }
        Ok(())
    }
}
