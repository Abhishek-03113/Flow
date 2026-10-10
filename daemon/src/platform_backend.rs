//! The seam between the engine and the operating system. The engine only
//! ever talks to a [`PlatformBackend`]: [`NativeBackend`] drives the real
//! per-OS adapters in `flow-platform`, while a virtual backend (the test
//! harness) can stand in for any OS with no input devices at all.

use std::sync::mpsc;

use flow_core::device::{HostOs, ScreenSize};
use flow_core::input::{InputCapture, InputInjector};
use flow_core::protocol::InputEvent;
use flow_platform::{new_default_input_injector, DefaultInputCapture};

pub type BackendError = Box<dyn std::error::Error + Send + Sync>;

/// A running capture, held by whoever started it.
pub trait CaptureControl: Send {
    /// Hides (or restores) the captured input from this machine's own
    /// applications; see `flow_core::input::InputCapture::set_suppress_local`.
    fn set_suppress_local(&mut self, suppress: bool) -> Result<(), BackendError>;
    fn stop(&mut self);
}

/// Replays events into this machine's input stream.
pub trait Injector: Send {
    fn inject(&mut self, event: &InputEvent) -> Result<(), BackendError>;
}

pub trait PlatformBackend: Send + Sync {
    fn host_os(&self) -> HostOs;
    fn screen_size(&self) -> ScreenSize;
    /// The pointer's absolute position, for backends that can read it.
    fn cursor_position(&self) -> Option<(i32, i32)> {
        None
    }
    /// Starts capturing physical input into `events`.
    fn start_capture(
        &self,
        events: mpsc::Sender<InputEvent>,
    ) -> Result<Box<dyn CaptureControl>, BackendError>;
    fn new_injector(&self) -> Result<Box<dyn Injector>, BackendError>;
}

/// The real adapters for the OS this binary was built for.
pub struct NativeBackend;

/// The OS this binary was compiled for.
pub fn native_host_os() -> HostOs {
    match std::env::consts::OS {
        "macos" => HostOs::Macos,
        "windows" => HostOs::Windows,
        _ => HostOs::Linux,
    }
}

impl PlatformBackend for NativeBackend {
    fn host_os(&self) -> HostOs {
        native_host_os()
    }

    fn screen_size(&self) -> ScreenSize {
        let (width, height) = flow_platform::screen_size()
            .or_else(screen_size_from_env)
            .unwrap_or((1920, 1080));
        ScreenSize { width, height }
    }

    fn cursor_position(&self) -> Option<(i32, i32)> {
        flow_platform::cursor_position()
    }

    fn start_capture(
        &self,
        events: mpsc::Sender<InputEvent>,
    ) -> Result<Box<dyn CaptureControl>, BackendError> {
        let mut capture = DefaultInputCapture::new(events);
        capture.start().map_err(|err| format!("{err:?}"))?;
        Ok(Box::new(NativeCapture(capture)))
    }

    /// The real injector can be thread-affine (macOS wraps a
    /// `CGEventSource`), so it lives on its own thread and the returned
    /// handle just forwards events to it.
    fn new_injector(&self) -> Result<Box<dyn Injector>, BackendError> {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (events_tx, events_rx) = mpsc::channel::<InputEvent>();

        std::thread::spawn(move || {
            let mut injector = match new_default_input_injector() {
                Ok(injector) => injector,
                Err(err) => {
                    let _ = ready_tx.send(Err(err.to_string()));
                    return;
                }
            };
            if ready_tx.send(Ok(())).is_err() {
                return;
            }
            for event in events_rx {
                if let Err(err) = injector.inject(&event) {
                    crate::logging::product::error("inject input", &err);
                }
            }
        });

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Box::new(ThreadedInjector(events_tx))),
            Ok(Err(message)) => Err(message.into()),
            Err(_) => Err("injector thread exited before it was ready".into()),
        }
    }
}

fn screen_size_from_env() -> Option<(u32, u32)> {
    let raw = std::env::var("FLOW_SCREEN_SIZE").ok()?;
    let (w, h) = raw.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

struct NativeCapture(DefaultInputCapture);

impl CaptureControl for NativeCapture {
    fn set_suppress_local(&mut self, suppress: bool) -> Result<(), BackendError> {
        self.0
            .set_suppress_local(suppress)
            .map_err(|err| format!("{err:?}").into())
    }

    fn stop(&mut self) {
        let _ = self.0.stop();
    }
}

struct ThreadedInjector(mpsc::Sender<InputEvent>);

impl Injector for ThreadedInjector {
    fn inject(&mut self, event: &InputEvent) -> Result<(), BackendError> {
        self.0
            .send(event.clone())
            .map_err(|_| "injector thread has stopped".into())
    }
}
