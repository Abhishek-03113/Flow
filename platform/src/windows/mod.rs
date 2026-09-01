//! Windows input adapter, bound to `WH_KEYBOARD_LL`/`WH_MOUSE_LL` hooks
//! for capture (`daemon/todos.json` E6) and `SendInput` for injection
//! (E7).

mod capture;
mod inject_translate;
mod injector;
mod translate;

pub use capture::{WindowsCaptureError, WindowsInputCapture};
pub use injector::{WindowsInjectError, WindowsInputInjector};

/// Stamped into the `dwExtraInfo` field of every `INPUT` this adapter
/// sends via `SendInput` ([`inject_translate::to_input`]) and checked by
/// the low-level hook procedures ([`capture`]) so they recognize — and
/// skip — this daemon's own injected output.
///
/// Without it, `WH_KEYBOARD_LL`/`WH_MOUSE_LL` re-capture every event this
/// process injects (they observe the whole system input stream,
/// synthetic events included). When this machine is the slave — receiving
/// a peer's input and injecting it — those re-captured events re-enter
/// the forwarding pipeline and, if the peer is the active device, get
/// sent straight back, echoing input between the two machines. For a
/// relative `MouseMove` the capture side also re-derives the delta from
/// the post-injection cursor position, so the echo compounds and the
/// deltas blow up. Linux avoids this by not enumerating its own uinput
/// node; macOS uses `EVENT_SOURCE_USER_DATA` the same way this constant is
/// used here. `0x466C6F77` is ASCII `"Flow"`; real hardware events carry
/// `0` here.
pub(crate) const FLOW_INJECTED_MARKER: usize = 0x466C_6F77;
