//! The seam between the engine and the operating system. The engine only
//! ever talks to a [`PlatformBackend`]: [`NativeBackend`] drives the real
//! per-OS adapters in `flow-platform`, while a virtual backend (the test
//! harness) can stand in for any OS with no input devices at all.

use std::sync::{mpsc, Arc};

use flow_core::device::{HostOs, ScreenSize};
use flow_core::input::{CaptureSink, InputCapture, InputInjector};
use flow_core::permission::PermissionItem;
use flow_core::protocol::InputEvent;
use flow_platform::{new_default_input_injector, DefaultInputCapture};

pub type BackendError = Box<dyn std::error::Error + Send + Sync>;

/// Keeps a capture running; dropping it stops the capture.
pub struct CaptureGuard(Option<Box<dyn FnOnce() + Send>>);

impl CaptureGuard {
    pub fn new(stop: impl FnOnce() + Send + 'static) -> Self {
        Self(Some(Box::new(stop)))
    }
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        if let Some(stop) = self.0.take() {
            stop();
        }
    }
}

/// Replays events into this machine's input stream.
pub trait Injector: Send {
    fn inject(&mut self, event: &InputEvent) -> Result<(), BackendError>;
    /// Moves the pointer to an absolute position, where the OS allows it.
    fn warp_cursor(&mut self, x: i32, y: i32) -> Result<(), BackendError>;
}

pub trait PlatformBackend: Send + Sync {
    fn host_os(&self) -> HostOs;
    fn screen_size(&self) -> ScreenSize;
    /// The pointer's absolute position, for backends that can read it.
    fn cursor_position(&self) -> Option<(i32, i32)> {
        None
    }
    /// Starts capturing physical input. `sink` decides synchronously, per
    /// event, whether local applications still see it.
    fn start_capture(&self, sink: Arc<dyn CaptureSink>) -> Result<CaptureGuard, BackendError>;
    fn new_injector(&self) -> Result<Box<dyn Injector>, BackendError>;

    /// What the OS currently allows. Empty (the default) means nothing
    /// needs granting, as on a virtual backend.
    fn permissions(&self) -> Vec<PermissionItem> {
        Vec::new()
    }
    /// Gets what is missing granted the best way the OS allows (macOS: its
    /// prompts; Linux: the one-time system setup behind one administrator
    /// prompt). May block until the person answers. `None` asks for
    /// everything still missing.
    fn request_permission(
        &self,
        _name: Option<&str>,
    ) -> Result<(), flow_platform::permissions::SetupError> {
        Ok(())
    }
    /// Whether [`Self::request_permission`] fixes every missing item by
    /// itself, behind a single prompt.
    fn permission_one_click_setup(&self) -> bool {
        false
    }
    /// Whether a permission granted while running applies only after a
    /// restart (macOS).
    fn permission_grant_needs_restart(&self) -> bool {
        false
    }
    /// Where the user grants `name` in the OS settings, if anywhere.
    fn permission_settings_url(&self, _name: &str) -> Option<String> {
        None
    }
}

/// The OS this binary was compiled for.
pub fn native_host_os() -> HostOs {
    match std::env::consts::OS {
        "macos" => HostOs::Macos,
        "windows" => HostOs::Windows,
        _ => HostOs::Linux,
    }
}

/// The real adapters for the OS this binary was built for.
pub struct NativeBackend;

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

    fn permissions(&self) -> Vec<PermissionItem> {
        flow_platform::permissions::check()
    }

    fn request_permission(
        &self,
        name: Option<&str>,
    ) -> Result<(), flow_platform::permissions::SetupError> {
        flow_platform::permissions::request(name)
    }

    fn permission_one_click_setup(&self) -> bool {
        flow_platform::permissions::one_click_setup()
    }

    fn permission_grant_needs_restart(&self) -> bool {
        flow_platform::permissions::grant_needs_restart()
    }

    fn permission_settings_url(&self, name: &str) -> Option<String> {
        flow_platform::permissions::settings_url(name).map(str::to_string)
    }

    fn start_capture(&self, sink: Arc<dyn CaptureSink>) -> Result<CaptureGuard, BackendError> {
        let mut capture = DefaultInputCapture::new(sink);
        capture.start().map_err(|err| format!("{err:?}"))?;
        Ok(CaptureGuard::new(move || {
            let _ = capture.stop();
        }))
    }

    /// The real injector can be thread-affine (macOS wraps a
    /// `CGEventSource`), so it lives on its own thread and the returned
    /// handle just forwards commands to it.
    fn new_injector(&self) -> Result<Box<dyn Injector>, BackendError> {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (commands_tx, commands_rx) = mpsc::channel::<InjectorCommand>();

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
            for command in commands_rx {
                let result = match command {
                    InjectorCommand::Event(event) => injector.inject(&event),
                    InjectorCommand::Warp(x, y) => injector.warp_cursor(x, y),
                };
                if let Err(err) = result {
                    crate::logging::product::error("inject input", &err);
                }
            }
        });

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Box::new(ThreadedInjector(commands_tx))),
            Ok(Err(message)) => Err(message.into()),
            Err(_) => Err("injector thread exited before it was ready".into()),
        }
    }
}

/// `FLOW_SCREEN_SIZE=WIDTHxHEIGHT`, for adapters that can't query the display.
fn screen_size_from_env() -> Option<(u32, u32)> {
    let raw = std::env::var("FLOW_SCREEN_SIZE").ok()?;
    let (w, h) = raw.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

enum InjectorCommand {
    Event(InputEvent),
    Warp(i32, i32),
}

struct ThreadedInjector(mpsc::Sender<InjectorCommand>);

impl Injector for ThreadedInjector {
    fn inject(&mut self, event: &InputEvent) -> Result<(), BackendError> {
        self.0
            .send(InjectorCommand::Event(event.clone()))
            .map_err(|_| "injector thread has stopped".into())
    }

    fn warp_cursor(&mut self, x: i32, y: i32) -> Result<(), BackendError> {
        self.0
            .send(InjectorCommand::Warp(x, y))
            .map_err(|_| "injector thread has stopped".into())
    }
}
