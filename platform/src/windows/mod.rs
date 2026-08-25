//! Windows input adapter, bound to `WH_KEYBOARD_LL`/`WH_MOUSE_LL` hooks
//! for capture (`daemon/todos.json` E6) and `SendInput` for injection
//! (E7).

mod capture;
mod inject_translate;
mod injector;
mod translate;

pub use capture::{WindowsCaptureError, WindowsInputCapture};
pub use injector::{WindowsInjectError, WindowsInputInjector};
