//! [`MacosInputCapture`]: installs a global `CGEventTap` and forwards
//! translated events over a channel.
//!
//! **Requires the Accessibility permission** (System Settings > Privacy &
//! Security > Accessibility) for the process calling `start()` —
//! `CGEventTapCreate` fails silently (returns a null tap, surfaced here as
//! `MacosCaptureError::TapCreationFailed`) rather than erroring loudly
//! without it. Surfacing that to the user as an actionable prompt is
//! track B's `request_permission` command's job once this adapter is
//! wired into it; this module only reports the failure.

use std::cell::RefCell;
use std::fmt;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
};
use flow_core::input::InputCapture;
use flow_core::protocol::InputEvent;

use super::translate::EventTranslator;

/// Every event type `EventTranslator` understands. Listing them
/// explicitly (rather than tapping everything) keeps the tap from paying
/// for event kinds Flow drops anyway.
fn events_of_interest() -> Vec<CGEventType> {
    vec![
        CGEventType::KeyDown,
        CGEventType::KeyUp,
        CGEventType::FlagsChanged,
        CGEventType::LeftMouseDown,
        CGEventType::LeftMouseUp,
        CGEventType::RightMouseDown,
        CGEventType::RightMouseUp,
        CGEventType::OtherMouseDown,
        CGEventType::OtherMouseUp,
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDragged,
        CGEventType::OtherMouseDragged,
        CGEventType::ScrollWheel,
    ]
}

#[derive(Debug)]
pub enum MacosCaptureError {
    /// `CGEventTapCreate` returned null — almost always a missing
    /// Accessibility permission grant, not a transient failure.
    TapCreationFailed,
    /// The run loop's mach port couldn't produce a run-loop source.
    RunLoopSourceFailed,
    /// The capture thread panicked; its state (and any resources it
    /// held) is unrecoverable.
    ThreadPanicked,
}

impl fmt::Display for MacosCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TapCreationFailed => write!(
                f,
                "CGEventTapCreate failed (commonly a missing Accessibility permission)"
            ),
            Self::RunLoopSourceFailed => {
                write!(f, "failed to create a run-loop source for the event tap")
            }
            Self::ThreadPanicked => write!(f, "the input capture thread panicked"),
        }
    }
}

impl std::error::Error for MacosCaptureError {}

/// Captures keyboard/mouse input via a `CGEventTap` and forwards it as
/// [`InputEvent`]s on the channel given at construction.
///
/// The `InputCapture` trait only has `start()`/`stop()` — no way to hand
/// back captured events — so the channel is supplied up front instead of
/// returned from `start()`, matching `LinuxInputCapture`.
pub struct MacosInputCapture {
    sender: Sender<InputEvent>,
    run_loop: Option<CFRunLoop>,
    worker: Option<JoinHandle<()>>,
}

impl MacosInputCapture {
    pub fn new(sender: Sender<InputEvent>) -> Self {
        Self {
            sender,
            run_loop: None,
            worker: None,
        }
    }
}

impl InputCapture for MacosInputCapture {
    type Error = MacosCaptureError;

    fn start(&mut self) -> Result<(), Self::Error> {
        if self.worker.is_some() {
            return Ok(());
        }

        let sender = self.sender.clone();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<CFRunLoop, MacosCaptureError>>();
        let worker = thread::spawn(move || run_capture_loop(sender, ready_tx));

        match ready_rx.recv() {
            Ok(Ok(run_loop)) => {
                self.run_loop = Some(run_loop);
                self.worker = Some(worker);
                Ok(())
            }
            Ok(Err(err)) => Err(err),
            // The sender end was dropped without a message: the thread
            // exited before reaching the point where it reports success.
            Err(_) => Err(MacosCaptureError::ThreadPanicked),
        }
    }

    fn stop(&mut self) -> Result<(), Self::Error> {
        if let Some(run_loop) = self.run_loop.take() {
            run_loop.stop();
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| MacosCaptureError::ThreadPanicked)?;
        }
        Ok(())
    }
}

fn run_capture_loop(
    sender: Sender<InputEvent>,
    ready: Sender<Result<CFRunLoop, MacosCaptureError>>,
) {
    let translator = RefCell::new(EventTranslator::new());
    let tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        events_of_interest(),
        move |_proxy, event_type, event| {
            let timestamp_ms = now_ms();
            if let Some(input_event) =
                translator
                    .borrow_mut()
                    .translate(event_type, event, timestamp_ms)
            {
                let _ = sender.send(input_event);
            }
            // ListenOnly: the return value is ignored by the OS, but
            // None signals "don't replace the event" for symmetry with
            // an active-filter tap.
            None
        },
    );
    let tap = match tap {
        Ok(tap) => tap,
        Err(()) => {
            let _ = ready.send(Err(MacosCaptureError::TapCreationFailed));
            return;
        }
    };

    let run_loop = CFRunLoop::get_current();
    let source = match tap.mach_port.create_runloop_source(0) {
        Ok(source) => source,
        Err(()) => {
            let _ = ready.send(Err(MacosCaptureError::RunLoopSourceFailed));
            return;
        }
    };
    // SAFETY: reads an `extern "C"` static (`kCFRunLoopCommonModes`); no
    // invariant beyond the crate having linked correctly.
    unsafe {
        run_loop.add_source(&source, kCFRunLoopCommonModes);
    }
    tap.enable();

    if ready.send(Ok(run_loop)).is_err() {
        // start() gave up waiting (its receiver was dropped) — nothing
        // left to hand events to.
        return;
    }

    CFRunLoop::run_current();
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}
