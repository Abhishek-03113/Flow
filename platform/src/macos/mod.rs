//! macOS input adapter, bound to a `CGEventTap` for capture
//! (`docs/tasks/daemon-todos.json` E4) and `CGEventPost` for injection (E5).

mod capture;
mod inject_translate;
mod injector;
mod translate;

pub use capture::{MacosCaptureError, MacosInputCapture};
pub use injector::{MacosInjectError, MacosInputInjector};

/// Stamped onto every `CGEvent` this adapter injects
/// ([`injector::MacosInputInjector`]), in the `EVENT_SOURCE_USER_DATA`
/// field, and checked by the capture event tap
/// ([`capture::MacosInputCapture`]) so it can recognize — and ignore —
/// its own output.
///
/// An active (non–listen-only) HID `CGEventTap` re-sees events posted by
/// this same process via `CGEventPost`. Without this marker the capture
/// side would forward the daemon's own injected input back to the peer
/// (an echo loop) and run it through the suppression gate. `0x466C6F77`
/// is ASCII `"Flow"`; the field defaults to `0`, so a real hardware
/// event never carries it.
pub(crate) const FLOW_INJECTED_MARKER: i64 = 0x466C_6F77;

pub(crate) fn screen_size() -> Option<(u32, u32)> {
    let size = core_graphics::display::CGDisplay::main().bounds().size;
    (size.width >= 1.0 && size.height >= 1.0).then_some((size.width as u32, size.height as u32))
}

pub(crate) fn cursor_position() -> Option<(i32, i32)> {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()?;
    let location = CGEvent::new(source).ok()?.location();
    Some((location.x as i32, location.y as i32))
}
