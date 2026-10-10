//! A [`PlatformBackend`] with no operating system behind it, so the engine
//! can run — several instances per process — in tests and the harness. It
//! can pretend to be any OS, lets the caller type on a virtual keyboard
//! and records both what local applications received and what the engine
//! injected.

use std::sync::{Arc, Mutex, MutexGuard};

use flow_core::device::{HostOs, ScreenSize};
use flow_core::input::{CaptureSink, Disposition};
use flow_core::protocol::InputEvent;

use crate::platform_backend::{BackendError, CaptureGuard, Injector, PlatformBackend};

#[derive(Default)]
struct Shared {
    sink: Mutex<Option<Arc<dyn CaptureSink>>>,
    delivered_locally: Mutex<Vec<InputEvent>>,
    injected: Mutex<Vec<InputEvent>>,
    warps: Mutex<Vec<(i32, i32)>>,
    cursor: Mutex<Option<(i32, i32)>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct VirtualBackend {
    host_os: HostOs,
    screen: ScreenSize,
    shared: Arc<Shared>,
}

impl VirtualBackend {
    pub fn new(host_os: HostOs, screen: ScreenSize) -> Self {
        Self {
            host_os,
            screen,
            shared: Arc::default(),
        }
    }

    /// A physical input event, as the OS would hand it to the capture
    /// hook. Whatever the engine lets through counts as delivered to local
    /// applications. `None` when no capture is running (the OS delivers it
    /// locally, unobserved).
    pub fn feed(&self, event: InputEvent) -> Option<Disposition> {
        let sink = lock(&self.shared.sink).clone()?;
        let disposition = sink.on_event(&event);
        if disposition == Disposition::Pass {
            lock(&self.shared.delivered_locally).push(event);
        }
        Some(disposition)
    }

    /// Attaches `service`'s router as this machine's capture, as the
    /// engine does for a controller.
    pub fn start_capture_for_router(&self, service: &crate::service::DaemonService) {
        let sink = Arc::clone(service.router()) as Arc<dyn CaptureSink>;
        *lock(&self.shared.sink) = Some(sink);
    }

    /// Moves the virtual pointer, for backends driving edge detection.
    pub fn set_cursor(&self, position: Option<(i32, i32)>) {
        *lock(&self.shared.cursor) = position;
    }

    /// Whether a capture is running.
    pub fn is_capturing(&self) -> bool {
        lock(&self.shared.sink).is_some()
    }

    /// Whether the engine currently hides physical input from local apps.
    pub fn is_suppressed(&self) -> bool {
        lock(&self.shared.sink)
            .as_ref()
            .is_some_and(|sink| sink.suppressing())
    }

    /// Physical events local applications received.
    pub fn delivered_locally(&self) -> Vec<InputEvent> {
        lock(&self.shared.delivered_locally).clone()
    }

    /// Everything the engine injected into this virtual machine so far.
    pub fn injected(&self) -> Vec<InputEvent> {
        lock(&self.shared.injected).clone()
    }

    /// Cursor warps the engine asked for, in order.
    pub fn warps(&self) -> Vec<(i32, i32)> {
        lock(&self.shared.warps).clone()
    }
}

impl PlatformBackend for VirtualBackend {
    fn host_os(&self) -> HostOs {
        self.host_os
    }

    fn screen_size(&self) -> ScreenSize {
        self.screen
    }

    fn cursor_position(&self) -> Option<(i32, i32)> {
        *lock(&self.shared.cursor)
    }

    fn start_capture(&self, sink: Arc<dyn CaptureSink>) -> Result<CaptureGuard, BackendError> {
        *lock(&self.shared.sink) = Some(sink);
        let shared = Arc::clone(&self.shared);
        Ok(CaptureGuard::new(move || {
            *lock(&shared.sink) = None;
        }))
    }

    fn new_injector(&self) -> Result<Box<dyn Injector>, BackendError> {
        Ok(Box::new(VirtualInjector(Arc::clone(&self.shared))))
    }
}

struct VirtualInjector(Arc<Shared>);

impl Injector for VirtualInjector {
    fn inject(&mut self, event: &InputEvent) -> Result<(), BackendError> {
        lock(&self.0.injected).push(event.clone());
        Ok(())
    }

    fn warp_cursor(&mut self, x: i32, y: i32) -> Result<(), BackendError> {
        lock(&self.0.warps).push((x, y));
        Ok(())
    }
}
