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
//!
//! The tap is created **active** (`CGEventTapOptions::Default`), not
//! listen-only, so that [`InputCapture::set_suppress_local`] can actually
//! withhold an event from this machine's own applications: while the
//! shared `suppress` flag is set, the tap callback returns a NULL
//! `CGEventRef` for events it consumes instead of the original pointer.
//! `core-graphics` 0.24's safe `CGEventTap` wrapper can't express that
//! (its trampoline maps a `None` return to "pass the original through"
//! and never returns NULL), so `run_capture_loop` calls `CGEventTapCreate`
//! directly through a small private [`ffi`] module with its own
//! `extern "C"` trampoline. All `unsafe` is contained in that trampoline,
//! `run_capture_loop`, and `mod ffi`.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::fmt;
use std::mem::ManuallyDrop;
use std::os::raw::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use core_foundation::base::TCFType;
use core_foundation::mach_port::{CFMachPort, CFMachPortRef};
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventMask, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType, CGKeyCode, EventField, KeyCode,
};
use core_graphics::sys::CGEventRef;
use flow_core::input::InputCapture;
use flow_core::protocol::InputEvent;
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
    /// The live local-suppression flag. Cloned into the capture thread's
    /// [`Context`] on `start()`; `set_suppress_local` flips it and every
    /// tap callback reads it. Held here too so the flag survives (and can
    /// be pre-set) across `stop()`/`start()`.
    suppress: Arc<AtomicBool>,
}

impl MacosInputCapture {
    pub fn new(sender: Sender<InputEvent>) -> Self {
        Self {
            sender,
            run_loop: None,
            worker: None,
            suppress: Arc::new(AtomicBool::new(false)),
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
        let suppress = Arc::clone(&self.suppress);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<CFRunLoop, MacosCaptureError>>();
        let worker = thread::spawn(move || run_capture_loop(sender, suppress, ready_tx));

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

    /// Flips the shared suppression flag every tap callback reads. While
    /// it is `true`, each callback still translates and forwards its
    /// event (so the active remote peer keeps receiving input) but then
    /// returns a NULL `CGEventRef` instead of the original event pointer,
    /// so the local OS never delivers it to this machine's own
    /// applications — see [`SuppressionGate`] for the press/release
    /// symmetry that keeps local key state consistent across a toggle.
    /// Safe to call before `start()` or after `stop()`: the flag simply
    /// carries over.
    ///
    /// **Unverified on real macOS hardware** — this needs the
    /// Accessibility permission and a live HID event stream, neither of
    /// which this project's CI has. See `daemon/README.md`'s "Local input
    /// suppression" section.
    fn set_suppress_local(&mut self, suppress: bool) -> Result<(), Self::Error> {
        self.suppress.store(suppress, Ordering::SeqCst);
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
    sender: Sender<InputEvent>,
    translator: RefCell<EventTranslator>,
    gate: RefCell<SuppressionGate>,
    /// Shared with [`MacosInputCapture`] on the caller's thread: written
    /// by `set_suppress_local`, read by every callback.
    suppress: Arc<AtomicBool>,
    /// The tap's mach port, stored once created so the callback can
    /// re-arm the tap after the OS disables it
    /// (`TapDisabledByTimeout` / `TapDisabledByUserInput`). Only ever
    /// touched on the run-loop thread — which is where every callback and
    /// the initial store both run — so a non-atomic `Cell` is enough.
    mach_port: Cell<CFMachPortRef>,
}

fn run_capture_loop(
    sender: Sender<InputEvent>,
    suppress: Arc<AtomicBool>,
    ready: Sender<Result<CFRunLoop, MacosCaptureError>>,
) {
    let ctx = Box::new(Context {
        sender,
        translator: RefCell::new(EventTranslator::new()),
        gate: RefCell::new(SuppressionGate::default()),
        suppress,
        mach_port: Cell::new(ptr::null_mut()),
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

        // Our own injected events (see `injector.rs`) come back around an
        // active HID tap. Recognize them by the marker and pass straight
        // through: don't forward Flow's own output to the peer, and don't
        // gate it.
        if cg.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA) == FLOW_INJECTED_MARKER {
            return false;
        }

        let timestamp_ms = now_ms();
        if let Some(input_event) = ctx
            .translator
            .borrow_mut()
            .translate(etype, cg, timestamp_ms)
        {
            // The peer ALWAYS gets the event, exactly as before
            // suppression existed.
            let _ = ctx.sender.send(input_event);
        }

        let suppress = ctx.suppress.load(Ordering::SeqCst);
        ctx.gate.borrow_mut().on_event(suppress, etype, cg)
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
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// The virtual keycode carried by a keyboard or `FlagsChanged` event.
fn keycode_of(event: &CGEvent) -> i64 {
    event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE)
}

/// The `CGEventFlags` bit a modifier keycode toggles, or `None` if `code`
/// isn't a modifier key. Mirrors `translate::modifier_for` +
/// `translate::flag_bit_for` so the gate reads a `FlagsChanged` press vs
/// release exactly the way `EventTranslator` does.
fn modifier_flag_bit(code: i64) -> Option<CGEventFlags> {
    match code as CGKeyCode {
        KeyCode::SHIFT | KeyCode::RIGHT_SHIFT => Some(CGEventFlags::CGEventFlagShift),
        KeyCode::CONTROL | KeyCode::RIGHT_CONTROL => Some(CGEventFlags::CGEventFlagControl),
        KeyCode::OPTION | KeyCode::RIGHT_OPTION => Some(CGEventFlags::CGEventFlagAlternate),
        KeyCode::COMMAND | KeyCode::RIGHT_COMMAND => Some(CGEventFlags::CGEventFlagCommand),
        _ => None,
    }
}

/// A stable id for the button an `OtherMouse*` event refers to.
/// `translate.rs` only models button number `2` (middle) — give it id `3`
/// to line up with the Windows gate's left=1 / right=2 / middle=3; any
/// rarer button keeps its raw number with the high bit set so it can
/// never collide with 1/2/3.
fn other_button_id(event: &CGEvent) -> u8 {
    match event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER) {
        2 => 3,
        other => (other as u8) | 0x80,
    }
}

/// Decides, per tap callback, whether an event must be withheld from the
/// local OS (callback returns NULL instead of the event pointer) while
/// this machine is forwarding input to an active remote peer.
///
/// The rule is *press/release symmetry*, not "withhold everything while
/// the flag is set": a release is withheld exactly when its matching
/// press was. That keeps local key/button state consistent across a
/// suppression toggle — a key pressed while local then held as
/// suppression turns on still gets its release delivered locally, and the
/// switch key's own key-up (which lands just after the daemon flips the
/// route and turns suppression on) does not strand a phantom key-down in
/// the local foreground app. Ported from `windows/capture.rs`'s
/// `SuppressionGate`.
#[derive(Default)]
struct SuppressionGate {
    /// Virtual keycodes whose key-down (or modifier press) this gate
    /// withheld and whose key-up must therefore be withheld too.
    withheld_keys: HashSet<i64>,
    /// Mouse-button ids (see [`other_button_id`]; left=1, right=2,
    /// middle=3) whose button-down this gate withheld.
    withheld_buttons: HashSet<u8>,
}

impl SuppressionGate {
    /// `suppress` is the current value of the shared local-suppression
    /// flag; `ty`/`event` are the tap callback's event type and event.
    fn on_event(&mut self, suppress: bool, ty: CGEventType, event: &CGEvent) -> bool {
        match ty {
            CGEventType::KeyDown => self.on_key(suppress, keycode_of(event), true),
            CGEventType::KeyUp => self.on_key(suppress, keycode_of(event), false),
            CGEventType::FlagsChanged => {
                let code = keycode_of(event);
                // Modifiers report a resulting flag bitmask, not a
                // discrete down/up — derive press vs release the same way
                // `EventTranslator::translate_flags_changed` does, then
                // run the keycode through the identical withheld-set logic
                // as a normal key.
                let pressed = match modifier_flag_bit(code) {
                    Some(bit) => event.get_flags().contains(bit),
                    // A non-modifier `FlagsChanged` (Caps Lock, Fn) has no
                    // flag bit the translator tracks and is dropped by
                    // translation anyway; fall back to "already tracked =>
                    // this is its release" so a withheld one still pairs.
                    None => !self.withheld_keys.contains(&code),
                };
                self.on_key(suppress, code, pressed)
            }
            CGEventType::LeftMouseDown => self.on_button(suppress, 1, true),
            CGEventType::LeftMouseUp => self.on_button(suppress, 1, false),
            CGEventType::RightMouseDown => self.on_button(suppress, 2, true),
            CGEventType::RightMouseUp => self.on_button(suppress, 2, false),
            CGEventType::OtherMouseDown => self.on_button(suppress, other_button_id(event), true),
            CGEventType::OtherMouseUp => self.on_button(suppress, other_button_id(event), false),
            // Motion and scroll have no press to pair a release with, so
            // they simply follow the current flag.
            CGEventType::MouseMoved
            | CGEventType::LeftMouseDragged
            | CGEventType::RightMouseDragged
            | CGEventType::OtherMouseDragged
            | CGEventType::ScrollWheel => suppress,
            // Anything else (tap-disabled notifications are handled before
            // the gate is ever consulted): never withhold.
            _ => false,
        }
    }

    fn on_key(&mut self, suppress: bool, code: i64, pressed: bool) -> bool {
        if pressed {
            if suppress {
                self.withheld_keys.insert(code);
                true
            } else {
                false
            }
        } else {
            // Withheld iff the matching press was. `remove` reports
            // whether it was tracked and clears it in one step.
            self.withheld_keys.remove(&code)
        }
    }

    fn on_button(&mut self, suppress: bool, id: u8, pressed: bool) -> bool {
        if pressed {
            if suppress {
                self.withheld_buttons.insert(id);
                true
            } else {
                false
            }
        } else {
            self.withheld_buttons.remove(&id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_graphics::event::CGMouseButton;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use core_graphics::geometry::CGPoint;

    fn source() -> CGEventSource {
        CGEventSource::new(CGEventSourceStateID::HIDSystemState).expect("event source")
    }

    fn key_event(code: CGKeyCode, is_down: bool) -> CGEvent {
        CGEvent::new_keyboard_event(source(), code, is_down).expect("keyboard event")
    }

    /// A `FlagsChanged` event for modifier `code`, with `flags` as the
    /// resulting bitmask — built exactly like `translate.rs`'s tests.
    fn flags_changed(code: CGKeyCode, flags: CGEventFlags) -> CGEvent {
        let event = key_event(code, true);
        event.set_flags(flags);
        event
    }

    fn mouse_event(ty: CGEventType, button: CGMouseButton) -> CGEvent {
        CGEvent::new_mouse_event(source(), ty, CGPoint::new(0.0, 0.0), button).expect("mouse event")
    }

    const VK_A: i64 = 0x00;
    const VK_B: i64 = 0x0B;

    #[test]
    fn with_suppression_off_no_key_event_is_withheld() {
        let mut gate = SuppressionGate::default();
        assert!(!gate.on_event(
            false,
            CGEventType::KeyDown,
            &key_event(VK_A as CGKeyCode, true)
        ));
        assert!(!gate.on_event(
            false,
            CGEventType::KeyUp,
            &key_event(VK_A as CGKeyCode, false)
        ));
    }

    #[test]
    fn with_suppression_on_a_full_key_press_and_release_are_both_withheld() {
        let mut gate = SuppressionGate::default();
        assert!(gate.on_event(
            true,
            CGEventType::KeyDown,
            &key_event(VK_A as CGKeyCode, true)
        ));
        assert!(gate.on_event(
            true,
            CGEventType::KeyUp,
            &key_event(VK_A as CGKeyCode, false)
        ));
    }

    #[test]
    fn a_key_release_is_withheld_after_suppression_is_handed_back_if_its_press_was() {
        let mut gate = SuppressionGate::default();
        // Pressed while forwarding to the peer...
        assert!(gate.on_event(
            true,
            CGEventType::KeyDown,
            &key_event(VK_A as CGKeyCode, true)
        ));
        // ...released after the route flipped back to local: still
        // withheld, so the local app never sees a dangling key-down.
        assert!(gate.on_event(
            false,
            CGEventType::KeyUp,
            &key_event(VK_A as CGKeyCode, false)
        ));
        // And exactly once.
        assert!(!gate.on_event(
            false,
            CGEventType::KeyUp,
            &key_event(VK_A as CGKeyCode, false)
        ));
    }

    #[test]
    fn a_key_release_reaches_local_when_its_press_did_even_if_suppression_turned_on_since() {
        let mut gate = SuppressionGate::default();
        // Pressed while local — the local app saw the key-down.
        assert!(!gate.on_event(
            false,
            CGEventType::KeyDown,
            &key_event(VK_A as CGKeyCode, true)
        ));
        // Suppression turns on mid-hold; the release must still reach the
        // local app or its key stays stuck down there.
        assert!(!gate.on_event(
            true,
            CGEventType::KeyUp,
            &key_event(VK_A as CGKeyCode, false)
        ));
    }

    #[test]
    fn keys_are_tracked_independently() {
        let mut gate = SuppressionGate::default();
        assert!(gate.on_event(
            true,
            CGEventType::KeyDown,
            &key_event(VK_A as CGKeyCode, true)
        ));
        assert!(gate.on_event(
            true,
            CGEventType::KeyDown,
            &key_event(VK_B as CGKeyCode, true)
        ));
        // Releasing A after the flag cleared: withheld (its down was);
        // B is still independently tracked.
        assert!(gate.on_event(
            false,
            CGEventType::KeyUp,
            &key_event(VK_A as CGKeyCode, false)
        ));
        assert!(gate.on_event(
            false,
            CGEventType::KeyUp,
            &key_event(VK_B as CGKeyCode, false)
        ));
        assert!(!gate.on_event(
            false,
            CGEventType::KeyUp,
            &key_event(VK_B as CGKeyCode, false)
        ));
    }

    #[test]
    fn a_flags_changed_press_and_release_follow_the_same_symmetry() {
        let mut gate = SuppressionGate::default();
        // Shift goes down (flag now set) while forwarding to the peer.
        let down = flags_changed(KeyCode::SHIFT, CGEventFlags::CGEventFlagShift);
        assert!(gate.on_event(true, CGEventType::FlagsChanged, &down));
        // Shift released (flag cleared) after the route flipped back:
        // still withheld because its press was, then not again.
        let up = flags_changed(KeyCode::SHIFT, CGEventFlags::CGEventFlagNull);
        assert!(gate.on_event(false, CGEventType::FlagsChanged, &up));
        let up_again = flags_changed(KeyCode::SHIFT, CGEventFlags::CGEventFlagNull);
        assert!(!gate.on_event(false, CGEventType::FlagsChanged, &up_again));
    }

    #[test]
    fn a_flags_changed_release_whose_press_reached_local_is_not_withheld() {
        let mut gate = SuppressionGate::default();
        // Control pressed while local — local app saw it.
        let down = flags_changed(KeyCode::CONTROL, CGEventFlags::CGEventFlagControl);
        assert!(!gate.on_event(false, CGEventType::FlagsChanged, &down));
        // Suppression turns on mid-hold; the release still reaches local.
        let up = flags_changed(KeyCode::CONTROL, CGEventFlags::CGEventFlagNull);
        assert!(!gate.on_event(true, CGEventType::FlagsChanged, &up));
    }

    #[test]
    fn mouse_move_and_scroll_follow_the_flag() {
        let mut gate = SuppressionGate::default();
        let moved = mouse_event(CGEventType::MouseMoved, CGMouseButton::Left);
        let scroll = mouse_event(CGEventType::ScrollWheel, CGMouseButton::Left);
        assert!(gate.on_event(true, CGEventType::MouseMoved, &moved));
        assert!(!gate.on_event(false, CGEventType::MouseMoved, &moved));
        assert!(gate.on_event(true, CGEventType::ScrollWheel, &scroll));
        assert!(!gate.on_event(false, CGEventType::ScrollWheel, &scroll));
    }

    #[test]
    fn a_mouse_button_release_is_withheld_iff_its_press_was() {
        let mut gate = SuppressionGate::default();
        let l_down = mouse_event(CGEventType::LeftMouseDown, CGMouseButton::Left);
        let l_up = mouse_event(CGEventType::LeftMouseUp, CGMouseButton::Left);
        let r_down = mouse_event(CGEventType::RightMouseDown, CGMouseButton::Right);
        let r_up = mouse_event(CGEventType::RightMouseUp, CGMouseButton::Right);
        // Pressed while forwarding, released after the route flipped back.
        assert!(gate.on_event(true, CGEventType::LeftMouseDown, &l_down));
        assert!(gate.on_event(false, CGEventType::LeftMouseUp, &l_up));
        assert!(!gate.on_event(false, CGEventType::LeftMouseUp, &l_up));
        // Pressed while local, suppression turns on mid-hold: the release
        // still reaches the local app.
        assert!(!gate.on_event(false, CGEventType::RightMouseDown, &r_down));
        assert!(!gate.on_event(true, CGEventType::RightMouseUp, &r_up));
    }

    #[test]
    fn mouse_buttons_are_tracked_independently_including_middle() {
        let mut gate = SuppressionGate::default();
        let l_down = mouse_event(CGEventType::LeftMouseDown, CGMouseButton::Left);
        let r_down = mouse_event(CGEventType::RightMouseDown, CGMouseButton::Right);
        let m_down = mouse_event(CGEventType::OtherMouseDown, CGMouseButton::Center);
        m_down.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, 2);
        let l_up = mouse_event(CGEventType::LeftMouseUp, CGMouseButton::Left);
        let r_up = mouse_event(CGEventType::RightMouseUp, CGMouseButton::Right);
        let m_up = mouse_event(CGEventType::OtherMouseUp, CGMouseButton::Center);
        m_up.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, 2);

        assert!(gate.on_event(true, CGEventType::LeftMouseDown, &l_down));
        assert!(gate.on_event(true, CGEventType::RightMouseDown, &r_down));
        assert!(gate.on_event(true, CGEventType::OtherMouseDown, &m_down));
        // Each release withheld once, independently.
        assert!(gate.on_event(false, CGEventType::LeftMouseUp, &l_up));
        assert!(gate.on_event(false, CGEventType::RightMouseUp, &r_up));
        assert!(gate.on_event(false, CGEventType::OtherMouseUp, &m_up));
        assert!(!gate.on_event(false, CGEventType::OtherMouseUp, &m_up));
    }

    #[test]
    fn event_mask_covers_the_real_types_and_skips_the_tap_disabled_sentinels() {
        let mask = event_mask();
        let key_down = 1u64 << CGEventType::KeyDown as u32;
        let scroll = 1u64 << CGEventType::ScrollWheel as u32;
        assert_eq!(mask & key_down, key_down);
        assert_eq!(mask & scroll, scroll);
        // 0xFFFF_FFFE / 0xFFFF_FFFF can't be represented as a shift and
        // must not have been folded in (that would have panicked or
        // wrapped); the mask stays a small bitset.
        assert_eq!(mask >> 28, 0);
    }
}
