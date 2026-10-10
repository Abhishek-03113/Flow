//! A [`PlatformBackend`] with no operating system behind it, so the engine
//! can run — several instances per process — in tests and the harness. It
//! can pretend to be any OS, lets the caller type on a virtual keyboard
//! and records what the engine injected.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use flow_core::device::{HostOs, ScreenSize};
use flow_core::protocol::InputEvent;

use crate::platform_backend::{BackendError, CaptureControl, Injector, PlatformBackend};

#[derive(Default)]
struct Shared {
    capture: Mutex<Option<mpsc::Sender<InputEvent>>>,
    suppressed: AtomicBool,
    injected: Mutex<Vec<InputEvent>>,
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

    /// A physical input event, as the OS would hand it to the capture hook.
    /// Returns whether a capture is running to receive it.
    pub fn feed(&self, event: InputEvent) -> bool {
        match &*self
            .shared
            .capture
            .lock()
            .unwrap_or_else(|e| e.into_inner())
        {
            Some(sender) => sender.send(event).is_ok(),
            None => false,
        }
    }

    /// Whether the engine currently hides physical input from local apps.
    pub fn is_suppressed(&self) -> bool {
        self.shared.suppressed.load(Ordering::SeqCst)
    }

    /// Everything the engine injected into this virtual machine so far.
    pub fn injected(&self) -> Vec<InputEvent> {
        self.shared
            .injected
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl PlatformBackend for VirtualBackend {
    fn host_os(&self) -> HostOs {
        self.host_os
    }

    fn screen_size(&self) -> ScreenSize {
        self.screen
    }

    fn start_capture(
        &self,
        events: mpsc::Sender<InputEvent>,
    ) -> Result<Box<dyn CaptureControl>, BackendError> {
        *self
            .shared
            .capture
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(events);
        Ok(Box::new(VirtualCapture(Arc::clone(&self.shared))))
    }

    fn new_injector(&self) -> Result<Box<dyn Injector>, BackendError> {
        Ok(Box::new(VirtualInjector(Arc::clone(&self.shared))))
    }
}

struct VirtualCapture(Arc<Shared>);

impl CaptureControl for VirtualCapture {
    fn set_suppress_local(&mut self, suppress: bool) -> Result<(), BackendError> {
        self.0.suppressed.store(suppress, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&mut self) {
        *self.0.capture.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

struct VirtualInjector(Arc<Shared>);

impl Injector for VirtualInjector {
    fn inject(&mut self, event: &InputEvent) -> Result<(), BackendError> {
        self.0
            .injected
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(event.clone());
        Ok(())
    }
}
