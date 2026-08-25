//! [`WindowsInputInjector`]: builds `INPUT` structs from incoming
//! `InputEvent`s and sends them via `SendInput`.

use std::fmt;
use std::mem::size_of;

use flow_core::input::InputInjector;
use flow_core::protocol::InputEvent;
use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};

use super::inject_translate::to_input;

#[derive(Debug)]
pub enum WindowsInjectError {
    /// `SendInput` reported it queued fewer events than were sent —
    /// per Win32 docs, this means another thread's input was already
    /// blocking the input stream (e.g. a UIPI-protected foreground
    /// window), not a transient failure worth retrying blindly.
    SendInputBlocked,
}

impl fmt::Display for WindowsInjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SendInputBlocked => {
                write!(f, "SendInput did not queue all events (input blocked)")
            }
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
            return Ok(());
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
