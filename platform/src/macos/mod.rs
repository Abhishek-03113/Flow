//! macOS input adapter, bound to a `CGEventTap` for capture
//! (`daemon/todos.json` E4) and `CGEventPost` for injection (E5).

mod capture;
mod inject_translate;
mod injector;
mod translate;

pub use capture::{MacosCaptureError, MacosInputCapture};
pub use injector::{MacosInjectError, MacosInputInjector};
