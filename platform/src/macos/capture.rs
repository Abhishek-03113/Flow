//! [`MacosInputCapture`]: installs a global active `CGEventTap` and hands
//! each translated event to the [`CaptureSink`].
//!
//! **Requires the Accessibility permission** (System Settings > Privacy &
//! Security > Accessibility) for the process calling `start()` —
//! `CGEventTapCreate` fails silently (a null tap, surfaced as
//! `MacosCaptureError::TapCreationFailed`) without it.
//!
//! The tap is *active*: the callback returns a NULL `CGEventRef` for events
//! the sink swallows, so they never reach local applications.
//! `core-graphics` 0.24's safe `CGEventTap` wrapper can't express that, so
//! `run_capture_loop` calls `CGEventTapCreate` directly through a small
//! private [`ffi`] module with its own `extern "C"` trampoline. All
//! `unsafe` is contained in that trampoline, `run_capture_loop`, and
//! `mod ffi`.
//!
//! While input is suppressed the cursor is also dissociated from the mouse
//! (`CGAssociateMouseAndMouseCursorPosition`) so the local pointer stays
//! parked; deltas keep arriving on the tap.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::mem::ManuallyDrop;
use std::os::raw::c_void;
use std::ptr;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use core_foundation::base::TCFType;
use core_foundation::mach_port::{CFMachPort, CFMachPortRef};
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEvent, CGEventMask, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType, EventField,
};
use core_graphics::sys::CGEventRef;
use flow_core::input::{CaptureSink, Disposition, InputCapture};
use foreign_types::ForeignType;

use super::translate::EventTranslator;
use super::FLOW_INJECTED_MARKER;

/// Every event type `EventTranslator` understands, plus the two
/// out-of-band tap-disabled notifications the callback re-arms on. Listing
/// the real types explicitly (rather than tapping everything) keeps the
/// tap from paying for event kinds Flow drops anyway.
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
        // Delivered to the callback whether or not they're in the mask
        // (see `event_mask`); listed here so the set of types the callback
        // must handle is stated in one place.
        CGEventType::TapDisabledByTimeout,
        CGEventType::TapDisabledByUserInput,
    ]
}

/// The `CGEventMask` bitmap for [`events_of_interest`]. `TapDisabledBy*`
/// have discriminants `0xFFFF_FFFE`/`0xFFFF_FFFF`, which are not valid
/// shift amounts and aren't representable as a mask bit — the OS delivers
/// them to an active tap's callback regardless, so they're simply skipped
/// when folding the mask.
fn event_mask() -> CGEventMask {
    events_of_interest()
        .into_iter()
        // `checked_shl` yields `None` for the `TapDisabledBy*` sentinels
        // (shift amount >= 64) and the real bit for every modeled type.
        .filter_map(|ty| 1u64.checked_shl(ty as u32))
        .fold(0, |mask, bit| mask | bit)
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

/// Captures keyboard/mouse input via a `CGEventTap`.
pub struct MacosInputCapture {
    sink: Arc<dyn CaptureSink>,
    run_loop: Option<CFRunLoop>,
    worker: Option<JoinHandle<()>>,
}

impl MacosInputCapture {
    pub fn new(sink: Arc<dyn CaptureSink>) -> Self {
        Self {
            sink,
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

        let sink = Arc::clone(&self.sink);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<CFRunLoop, MacosCaptureError>>();
        let worker = thread::spawn(move || run_capture_loop(sink, ready_tx));

        match ready_rx.recv() {
            Ok(Ok(run_loop)) => {
                self.run_loop = Some(run_loop);
                self.worker = Some(worker);
                Ok(())
            }
            Ok(Err(err)) => Err(err),
            // The thread exited before reporting success.
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

/// Heap state the raw event-tap trampoline reads on every callback. A
/// `*const Context` is handed to `CGEventTapCreate` as its `userInfo`;
/// the owning `Box` lives on `run_capture_loop`'s stack and is dropped
/// only after `CFRunLoop::run_current()` returns — which `stop()`
/// triggers, and which happens before the worker thread joins — so the
/// pointer is valid for the lifetime of every callback and the box is
/// freed on `stop()` with no manual `unsafe` free and no leak.
struct Context {
    sink: Arc<dyn CaptureSink>,
    translator: RefCell<EventTranslator>,
    /// The tap's mach port, stored once created so the callback can
    /// re-arm the tap after the OS disables it
    /// (`TapDisabledByTimeout` / `TapDisabledByUserInput`). Only touched
    /// on the run-loop thread, so a non-atomic `Cell` is enough.
    mach_port: Cell<CFMachPortRef>,
    /// Whether the cursor is currently dissociated from the mouse.
    cursor_parked: Cell<bool>,
}

fn run_capture_loop(
    sink: Arc<dyn CaptureSink>,
    ready: Sender<Result<CFRunLoop, MacosCaptureError>>,
) {
    let ctx = Box::new(Context {
        sink,
        translator: RefCell::new(EventTranslator::new()),
        mach_port: Cell::new(ptr::null_mut()),
        cursor_parked: Cell::new(false),
    });
    let ctx_ptr: *const Context = &*ctx;

    // SAFETY: `event_tap_callback` is a plain `extern "C"` fn matching the
    // signature `CGEventTapCreate` expects. `ctx_ptr` points at `ctx`, a
    // live `Box<Context>` on this stack frame that outlives every callback
    // (it drops only when this function returns, and this function only
    // returns after `stop()` has stopped the run loop below). The
    // location/placement/options are plain by-value C enums and `event_mask()`
    // is the folded `CGEventMask`.
    let mach_port_ref = unsafe {
        ffi::CGEventTapCreate(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            // Active tap (not `ListenOnly`): its callback may return NULL
            // to drop an event, which is how suppression withholds input.
            CGEventTapOptions::Default,
            event_mask(),
            event_tap_callback,
            ctx_ptr as *mut c_void,
        )
    };
    if mach_port_ref.is_null() {
        let _ = ready.send(Err(MacosCaptureError::TapCreationFailed));
        return;
    }
    // SAFETY: `CGEventTapCreate` returns its `CFMachPortRef` under the
    // Core Foundation create rule (the caller owns one reference); wrap it
    // so that reference is `CFRelease`d when `mach_port` drops at the end
    // of this function.
    let mach_port = unsafe { CFMachPort::wrap_under_create_rule(mach_port_ref) };
    ctx.mach_port.set(mach_port_ref);

    let source = match mach_port.create_runloop_source(0) {
        Ok(source) => source,
        Err(()) => {
            let _ = ready.send(Err(MacosCaptureError::RunLoopSourceFailed));
            return;
        }
    };

    let run_loop = CFRunLoop::get_current();
    // SAFETY: reads the `extern "C"` static `kCFRunLoopCommonModes`; the
    // source was just produced by `create_runloop_source` and is valid.
    unsafe {
        run_loop.add_source(&source, kCFRunLoopCommonModes);
    }
    // SAFETY: `mach_port_ref` is the live tap port from `CGEventTapCreate`
    // above and is still retained by `mach_port`; enabling an
    // already-enabled tap is a documented no-op.
    unsafe {
        ffi::CGEventTapEnable(mach_port_ref, true);
    }

    if ready.send(Ok(run_loop)).is_err() {
        // start() gave up waiting (its receiver was dropped) — nothing
        // left to hand events to.
        return;
    }

    CFRunLoop::run_current();
    // Reached only after `stop()` called `run_loop.stop()`, so the run
    // loop has stopped servicing the tap and no further callback can run.
    // Never leave the pointer parked on the way out.
    set_cursor_parked(&ctx, false);
    // `ctx`, `mach_port`, and `source` drop here.
}

/// The raw event-tap callback. Contract with `CGEventTapCreate`: return
/// the event pointer to let the event through to the local OS, or a NULL
/// `CGEventRef` to drop it. Always runs on the worker thread's run loop,
/// never concurrently with itself.
///
/// Fails **open**: any panic, borrow conflict, or unexpected state ends
/// with the event passed through untouched. Trapping the user's own
/// keyboard or mouse because of a bug in here is never acceptable — at
/// worst this drops or double-delivers a single event, mirroring how the
/// Windows hook's `guard_hook_body` degrades.
unsafe extern "C" fn event_tap_callback(
    _proxy: CGEventTapProxy,
    etype: CGEventType,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef {
    // SAFETY: `user_info` is the `*const Context` passed to
    // `CGEventTapCreate` in `run_capture_loop`; that `Box<Context>` is
    // still alive (it drops only after the run loop stops) and this
    // callback is never re-entered concurrently, so a shared reference is
    // sound. `as_ref` still guards defensively against a null pointer.
    let Some(ctx) = (unsafe { user_info.cast::<Context>().as_ref() }) else {
        return event;
    };

    // Out-of-band: the OS disabled the tap (a callback that ran too long,
    // or a burst of input). Re-arm it and pass through — nothing to
    // translate or gate.
    if matches!(
        etype,
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
    ) {
        let port = ctx.mach_port.get();
        if !port.is_null() {
            // SAFETY: `port` is the tap's `CFMachPortRef`, still retained
            // by `mach_port` in `run_capture_loop` for the run loop's life.
            unsafe { ffi::CGEventTapEnable(port, true) };
        }
        return event;
    }

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: for a normal event callback `event` is a valid
        // `CGEventRef` for the duration of this call. `ManuallyDrop`
        // borrows it without taking ownership, so returning here never
        // `CFRelease`s a pointer this callback doesn't own.
        let borrowed = ManuallyDrop::new(unsafe { CGEvent::from_ptr(event) });
        let cg: &CGEvent = &borrowed;

        // Our own injected events come back around an active HID tap;
        // they are not physical input.
        if cg.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA) == FLOW_INJECTED_MARKER {
            return false;
        }

        let timestamp_ms = now_ms();
        let swallow = match ctx
            .translator
            .borrow_mut()
            .translate(etype, cg, timestamp_ms)
        {
            Some(input_event) => ctx.sink.on_event(&input_event) == Disposition::Swallow,
            None => ctx.sink.suppressing(),
        };
        set_cursor_parked(ctx, ctx.sink.suppressing());
        swallow
    }));

    match outcome {
        Ok(true) => ptr::null_mut(),
        Ok(false) => event,
        Err(_) => {
            // A tracing macro is panic-safe; don't propagate — an
            // unwinding panic across this `extern "C"` frame is an
            // immediate `abort()`.
            tracing::error!("the macOS event-tap callback panicked; the event was passed through");
            event
        }
    }
}

/// Raw Core Graphics event-tap FFI. Re-declared here rather than reused
/// from `core-graphics` because that crate's safe `CGEventTap` wrapper can
/// only build a pass-through callback (`None` return -> original event),
/// never one that drops an event by returning NULL, which is exactly what
/// local suppression needs.
mod ffi {
    use core_foundation::mach_port::CFMachPortRef;
    use core_graphics::event::{
        CGEventMask, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy,
        CGEventType,
    };
    use core_graphics::sys::CGEventRef;
    use std::os::raw::c_void;

    /// The C callback signature `CGEventTapCreate` expects: the event
    /// arrives as a raw `CGEventRef`, and the return value is the
    /// `CGEventRef` to forward — the same pointer to pass the event
    /// through, or NULL to drop it.
    pub type CGEventTapCallBack = unsafe extern "C" fn(
        proxy: CGEventTapProxy,
        etype: CGEventType,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef;

    // SAFETY: signatures transcribed from `<CoreGraphics/CGEvent.h>`. The
    // CoreGraphics framework itself is already linked by the `core-graphics`
    // crate (its default `link` feature); this `#[link]` is redundant with
    // that and only names the two symbols that crate keeps private.
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        /// Creates an event tap. Returns a `CFMachPortRef` under the
        /// create rule, or NULL on failure (almost always a missing
        /// Accessibility grant).
        pub fn CGEventTapCreate(
            tap: CGEventTapLocation,
            place: CGEventTapPlacement,
            options: CGEventTapOptions,
            events_of_interest: CGEventMask,
            callback: CGEventTapCallBack,
            user_info: *mut c_void,
        ) -> CFMachPortRef;

        /// Enables or disables an event tap. Used both to arm the tap
        /// initially and to re-arm it from the callback after the OS
        /// disables it.
        pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);

        /// Connects (non-zero) or disconnects (zero) mouse movement from
        /// the on-screen cursor. Returns a `CGError` (0 = success).
        pub fn CGAssociateMouseAndMouseCursorPosition(connected: i32) -> i32;
    }
}

/// Parks (dissociates) or releases the cursor, only when that changes.
fn set_cursor_parked(ctx: &Context, parked: bool) {
    if ctx.cursor_parked.replace(parked) != parked {
        // SAFETY: a plain CoreGraphics call with no pointer arguments.
        unsafe {
            ffi::CGAssociateMouseAndMouseCursorPosition(i32::from(!parked));
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}
