//! Manual, local-only loopback sanity check for `flow-platform`'s Linux
//! input adapters (`daemon/todos.json` E3): captures real keyboard/mouse
//! input and immediately re-injects it into a Flow-owned virtual device,
//! the minimum way to exercise `LinuxInputCapture` (E1) and
//! `LinuxInputInjector` (E2) together on one machine, without a second
//! device or the full daemon/networking stack.
//!
//! Needs `/dev/input` (read) and `/dev/uinput` (write) access — see
//! `daemon/README.md`'s "E1: Linux capture via evdev" and "E2: Linux
//! injection via uinput" manual verification notes; this repo's own
//! development container has neither, so this example is unverified
//! beyond `cargo build --example linux_input_echo`. Run with:
//!
//! ```sh
//! cargo run -p flow-daemon --example linux_input_echo
//! ```
//!
//! Prints every captured event and hands it straight to the virtual
//! device (watch it with `evtest` on the new `/dev/input/eventN` node
//! named "Flow Virtual Input"). Not part of the automated test suite —
//! kill the process (Ctrl+C) to stop it.

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    use std::sync::mpsc;

    use flow_core::input::{InputCapture, InputInjector};
    use flow_platform::{LinuxInputCapture, LinuxInputInjector};

    let (sender, receiver) = mpsc::channel();
    let mut capture = LinuxInputCapture::new(sender);
    let mut injector = LinuxInputInjector::new()?;

    capture.start()?;
    println!("Capturing and looping back input onto \"Flow Virtual Input\". Ctrl+C to stop.");

    for event in receiver {
        println!("{event:?}");
        if let Err(err) = injector.inject(&event) {
            eprintln!("injection failed: {err}");
        }
    }

    capture.stop()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("linux_input_echo is Linux-only (it binds to evdev/uinput)");
}
