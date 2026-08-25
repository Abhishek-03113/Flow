//! Linux input adapter, bound to evdev for capture (`daemon/todos.json`
//! E1) and uinput for injection (E2).

mod capture;
mod discovery;
mod inject_translate;
mod injector;
mod translate;

pub use capture::LinuxInputCapture;
pub use injector::LinuxInputInjector;
