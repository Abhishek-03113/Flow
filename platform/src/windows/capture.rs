//! [`WindowsInputCapture`]: installs `WH_KEYBOARD_LL`/`WH_MOUSE_LL` hooks
//! and forwards translated events over a channel.
//!
//! Low-level hooks require a message loop pumping on the thread that
//! installed them — the OS delivers hook callbacks through that thread's
//! message queue — so `start()` spawns a dedicated thread for both the
//! hooks and the loop, mirroring `LinuxInputCapture`/`MacosInputCapture`.
//! Hook procedures are plain `extern "system"` function pointers with no
//! user-data parameter, so the translator and output channel live in
//! thread-local storage instead, populated before the hooks are
//! installed and cleared after the message loop exits — safe because
//! Windows always calls a low-level hook's procedure on the thread that
//! registered it.
//!
//! Local suppression ([`InputCapture::set_suppress_local`]) is a shared
//! [`AtomicBool`], not part of that thread-local state, because the
//! writer is the daemon's pipeline task — a *different* thread than the
//! one the hooks run on. While it is set, each hook callback still
//! translates and forwards its event to the active peer, then returns
//! `LRESULT(1)` instead of chaining to `CallNextHookEx`, so the local OS
//! never delivers it here. A [`SuppressionGate`] in the thread-local
//! state tracks which presses were withheld so the matching release is
//! withheld too even across a mid-hold toggle — which is what stops the
//! switch key's own key-up stranding a phantom key-down in the local
//! foreground app.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use flow_core::input::InputCapture;
use flow_core::protocol::InputEvent;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use super::translate::EventTranslator;
use super::FLOW_INJECTED_MARKER;

thread_local! {
    static STATE: RefCell<Option<CaptureState>> = const { RefCell::new(None) };
}

struct CaptureState {
    translator: EventTranslator,
    sender: Sender<InputEvent>,
    /// Shared with [`WindowsInputCapture`] on the caller's thread: set by
    /// `set_suppress_local`, read by every hook callback. `AtomicBool`
    /// rather than part of this thread-local state precisely because the
    /// writer runs on a different thread than the hooks.
    suppress: Arc<AtomicBool>,
    /// Per-thread book-keeping for press/release symmetry — only ever
    /// touched by the hook callbacks, so it stays in the thread-local
    /// state rather than being shared.
    gate: SuppressionGate,
}

#[derive(Debug)]
pub enum WindowsCaptureError {
    /// `SetWindowsHookExW` failed for `WH_KEYBOARD_LL` or `WH_MOUSE_LL`.
    HookInstallFailed,
    /// The capture thread panicked; its state (and its hooks) is
    /// unrecoverable.
    ThreadPanicked,
}

impl fmt::Display for WindowsCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HookInstallFailed => write!(f, "SetWindowsHookExW failed"),
            Self::ThreadPanicked => write!(f, "the input capture thread panicked"),
        }
    }
}

impl std::error::Error for WindowsCaptureError {}

/// Captures keyboard/mouse input via low-level hooks and forwards it as
/// [`InputEvent`]s on the channel given at construction.
///
/// The `InputCapture` trait only has `start()`/`stop()` — no way to hand
/// back captured events — so the channel is supplied up front instead of
/// returned from `start()`, matching the Linux and macOS adapters.
pub struct WindowsInputCapture {
    sender: Sender<InputEvent>,
    capture_thread_id: Option<u32>,
    worker: Option<JoinHandle<()>>,
    /// The live local-suppression flag. Cloned into the capture thread's
    /// [`CaptureState`] on `start()`; `set_suppress_local` flips it and
    /// every hook callback reads it. Held here too so the flag survives
    /// (and can be pre-set) across `stop()`/`start()`.
    suppress: Arc<AtomicBool>,
}

impl WindowsInputCapture {
    pub fn new(sender: Sender<InputEvent>) -> Self {
        Self {
            sender,
            capture_thread_id: None,
            worker: None,
            suppress: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl InputCapture for WindowsInputCapture {
    type Error = WindowsCaptureError;

    fn start(&mut self) -> Result<(), Self::Error> {
        if self.worker.is_some() {
            return Ok(());
        }

        let sender = self.sender.clone();
        let suppress = Arc::clone(&self.suppress);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, WindowsCaptureError>>();
        let worker = thread::spawn(move || run_capture_loop(sender, suppress, ready_tx));

        match ready_rx.recv() {
            Ok(Ok(thread_id)) => {
                self.capture_thread_id = Some(thread_id);
                self.worker = Some(worker);
                Ok(())
            }
            Ok(Err(err)) => Err(err),
            // The sender end was dropped without a message: the thread
            // exited before reaching the point where it reports success.
            Err(_) => Err(WindowsCaptureError::ThreadPanicked),
        }
    }

    fn stop(&mut self) -> Result<(), Self::Error> {
        if let Some(thread_id) = self.capture_thread_id.take() {
            // SAFETY: posts WM_QUIT to unblock that thread's GetMessageW
            // loop; a plain FFI call with no aliasing/lifetime concerns.
            unsafe {
                let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| WindowsCaptureError::ThreadPanicked)?;
        }
        Ok(())
    }

    /// Flips the shared suppression flag every hook callback reads. While
    /// it is `true`, each callback still translates and forwards its
    /// event (so the active remote peer keeps receiving input) but then
    /// returns `LRESULT(1)` instead of chaining to `CallNextHookEx`, so
    /// the local OS never delivers it to this machine's own applications
    /// — see [`SuppressionGate`] for the press/release symmetry that
    /// keeps local key state consistent across a toggle. Safe to call
    /// before `start()` or after `stop()`: the flag simply carries over.
    fn set_suppress_local(&mut self, suppress: bool) -> Result<(), Self::Error> {
        self.suppress.store(suppress, Ordering::SeqCst);
        Ok(())
    }
}

fn run_capture_loop(
    sender: Sender<InputEvent>,
    suppress: Arc<AtomicBool>,
    ready: Sender<Result<u32, WindowsCaptureError>>,
) {
    STATE.with(|state| {
        *state.borrow_mut() = Some(CaptureState {
            translator: EventTranslator::new(),
            sender,
            suppress,
            gate: SuppressionGate::default(),
        });
    });

    // SAFETY: hmod=None and dwthreadid=0 is the documented form for a
    // low-level hook installed by a thread in the current process (no
    // module handle needed); the callbacks are `'static` function
    // pointers with no captured state.
    let keyboard_hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0) };
    let Ok(keyboard_hook) = keyboard_hook else {
        let _ = ready.send(Err(WindowsCaptureError::HookInstallFailed));
        STATE.with(|state| *state.borrow_mut() = None);
        return;
    };

    let mouse_hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) };
    let Ok(mouse_hook) = mouse_hook else {
        unsafe {
            let _ = UnhookWindowsHookEx(keyboard_hook);
        }
        let _ = ready.send(Err(WindowsCaptureError::HookInstallFailed));
        STATE.with(|state| *state.borrow_mut() = None);
        return;
    };

    // SAFETY: reads the calling thread's own id; no preconditions.
    let thread_id = unsafe { GetCurrentThreadId() };
    if ready.send(Ok(thread_id)).is_err() {
        // start() gave up waiting (its receiver was dropped) — nothing
        // left to hand events to.
        unhook(keyboard_hook, mouse_hook);
        STATE.with(|state| *state.borrow_mut() = None);
        return;
    }

    let mut message = MSG::default();
    // SAFETY: `message` is a valid, uniquely-owned MSG for the duration
    // of each call; GetMessageW/DispatchMessageW's usual FFI contract.
    // GetMessageW returns >0 for a normal message, 0 for WM_QUIT, and a
    // negative value on error — only >0 means "keep pumping".
    unsafe {
        while GetMessageW(&mut message, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    unhook(keyboard_hook, mouse_hook);
    STATE.with(|state| *state.borrow_mut() = None);
}

fn unhook(keyboard_hook: HHOOK, mouse_hook: HHOOK) {
    // SAFETY: both handles came from a successful SetWindowsHookExW
    // earlier in this same function and haven't been unhooked yet.
    unsafe {
        let _ = UnhookWindowsHookEx(keyboard_hook);
        let _ = UnhookWindowsHookEx(mouse_hook);
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: for code >= 0, lparam points to a valid KBDLLHOOKSTRUCT
        // for the duration of this call, per WH_KEYBOARD_LL's contract.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        // This daemon's own injected output (see `super::FLOW_INJECTED_MARKER`)
        // is observed by this hook like any other event. Skip it entirely
        // — don't translate, forward, or gate it — or a slave machine
        // re-forwards the peer's input straight back and the two echo.
        if info.dwExtraInfo == FLOW_INJECTED_MARKER {
            // SAFETY: pass-through to the next hook, WH_KEYBOARD_LL contract.
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }
        let message = wparam.0 as u32;
        let vk_code = info.vkCode;
        let timestamp_ms = now_ms();
        let withhold = guard_hook_body("keyboard", || {
            STATE.with(|state| {
                let mut slot = state.borrow_mut();
                let Some(state) = slot.as_mut() else {
                    return false;
                };
                if let Some(event) =
                    state
                        .translator
                        .translate_keyboard(message, info, timestamp_ms)
                {
                    let _ = state.sender.send(event);
                }
                let suppress = state.suppress.load(Ordering::SeqCst);
                state.gate.on_keyboard(suppress, message, vk_code)
            })
        });
        if withhold {
            // Break the hook chain so the local OS never delivers this
            // event to a foreground application — it has already been
            // forwarded to the active remote peer just above.
            return LRESULT(1);
        }
    }
    // SAFETY: forwards to the next hook in the chain, as required by the
    // WH_KEYBOARD_LL contract when this callback did not withhold the event.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: for code >= 0, lparam points to a valid MSLLHOOKSTRUCT
        // for the duration of this call, per WH_MOUSE_LL's contract.
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        // Skip this daemon's own injected output — see the matching note
        // in `keyboard_proc` and `super::FLOW_INJECTED_MARKER`.
        if info.dwExtraInfo == FLOW_INJECTED_MARKER {
            // SAFETY: pass-through to the next hook, WH_MOUSE_LL contract.
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }
        let message = wparam.0 as u32;
        let timestamp_ms = now_ms();
        let withhold = guard_hook_body("mouse", || {
            STATE.with(|state| {
                let mut slot = state.borrow_mut();
                let Some(state) = slot.as_mut() else {
                    return false;
                };
                if let Some(event) = state
                    .translator
                    .translate_mouse(message, info, timestamp_ms)
                {
                    let _ = state.sender.send(event);
                }
                let suppress = state.suppress.load(Ordering::SeqCst);
                state.gate.on_mouse(suppress, message)
            })
        });
        if withhold {
            return LRESULT(1);
        }
    }
    // SAFETY: forwards to the next hook in the chain, as required by the
    // WH_MOUSE_LL contract when this callback did not withhold the event.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Runs one hook callback's translate-forward-and-gate body, catching any
/// panic before it can reach the `extern "system"` frame — where an
/// unwinding panic becomes an immediate `abort()` and takes the whole
/// daemon down. A bug in translation, the gate, or a hostile/degenerate
/// hook struct must at worst drop one event, matching how the daemon
/// degrades everywhere else. `AssertUnwindSafe` is sound here because a
/// caught panic leaves nothing observably broken: the `RefCell` borrow
/// is released as its `RefMut` unwinds, and the worst outcome is a
/// missed event or a translator whose `last_mouse_position` is one step
/// stale.
///
/// Returns whether the event should be withheld from the local OS. On a
/// caught panic it returns `false` — fail open, never trap the user's own
/// keyboard or mouse because of a bug in here.
fn guard_hook_body(which: &str, body: impl FnOnce() -> bool) -> bool {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(withhold) => withhold,
        Err(_) => {
            // Note, don't propagate. A tracing macro is panic-safe.
            tracing::error!("the {which} hook callback panicked; the event was dropped");
            false
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn is_key_down(message: u32) -> bool {
    message == WM_KEYDOWN || message == WM_SYSKEYDOWN
}

fn is_key_up(message: u32) -> bool {
    message == WM_KEYUP || message == WM_SYSKEYUP
}

/// A small stable id for the mouse button a `WM_*BUTTONDOWN`/`UP` message
/// refers to, or `None` for a non-button mouse message (move, wheel).
/// `XBUTTON1`/`XBUTTON2` share one id — the low-level hook distinguishes
/// them only via `mouseData`, and suppression doesn't need to.
fn mouse_button_id(message: u32) -> Option<u8> {
    if message == WM_LBUTTONDOWN || message == WM_LBUTTONUP {
        Some(1)
    } else if message == WM_RBUTTONDOWN || message == WM_RBUTTONUP {
        Some(2)
    } else if message == WM_MBUTTONDOWN || message == WM_MBUTTONUP {
        Some(3)
    } else if message == WM_XBUTTONDOWN || message == WM_XBUTTONUP {
        Some(4)
    } else {
        None
    }
}

fn is_button_down(message: u32) -> bool {
    message == WM_LBUTTONDOWN
        || message == WM_RBUTTONDOWN
        || message == WM_MBUTTONDOWN
        || message == WM_XBUTTONDOWN
}

fn is_button_up(message: u32) -> bool {
    message == WM_LBUTTONUP
        || message == WM_RBUTTONUP
        || message == WM_MBUTTONUP
        || message == WM_XBUTTONUP
}

/// Decides, per hook callback, whether an event should be withheld from
/// the local OS (return `LRESULT(1)` instead of chaining to
/// `CallNextHookEx`) while this machine is forwarding input to an active
/// remote peer.
///
/// The rule is *press/release symmetry*, not simply "withhold everything
/// while the flag is set": a release is withheld exactly when its
/// matching press was. That keeps the local key/button state consistent
/// across a suppression toggle — a key pressed while local then held as
/// suppression turns on still gets its release delivered locally, and the
/// switch key's own key-up (which lands just after the daemon flips the
/// route and turns suppression on) does not strand a phantom key-down in
/// the local foreground app.
#[derive(Default)]
struct SuppressionGate {
    /// Virtual-key codes whose key-down this gate withheld and whose
    /// key-up must therefore be withheld too.
    withheld_keys: HashSet<u32>,
    /// Mouse-button ids (see [`mouse_button_id`]) whose button-down this
    /// gate withheld.
    withheld_buttons: HashSet<u8>,
}

impl SuppressionGate {
    /// `suppress` is the current value of the shared local-suppression
    /// flag; `message` is the hook's `wparam` (`WM_KEYDOWN` etc.);
    /// `vk_code` is `KBDLLHOOKSTRUCT::vkCode`.
    fn on_keyboard(&mut self, suppress: bool, message: u32, vk_code: u32) -> bool {
        if is_key_down(message) {
            if suppress {
                self.withheld_keys.insert(vk_code);
                true
            } else {
                false
            }
        } else if is_key_up(message) {
            // Withheld iff the matching down was withheld. `remove`
            // reports whether it was tracked, and clears it in one step.
            self.withheld_keys.remove(&vk_code)
        } else {
            // No such message reaches a `WH_KEYBOARD_LL` hook in
            // practice; fall back to the raw flag rather than leak.
            suppress
        }
    }

    /// `message` is the hook's `wparam` (`WM_MOUSEMOVE`, `WM_LBUTTONDOWN`,
    /// `WM_MOUSEWHEEL`, ...).
    fn on_mouse(&mut self, suppress: bool, message: u32) -> bool {
        if is_button_down(message) {
            if suppress {
                if let Some(id) = mouse_button_id(message) {
                    self.withheld_buttons.insert(id);
                }
                true
            } else {
                false
            }
        } else if is_button_up(message) {
            match mouse_button_id(message) {
                Some(id) => self.withheld_buttons.remove(&id),
                None => suppress,
            }
        } else {
            // Move / wheel / hwheel: no press to pair a release with, so
            // they simply follow the current flag.
            suppress
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Virtual-key codes used by the gate tests. Bare u32s — the gate
    // never interprets them, it only compares.
    const VK_A: u32 = 0x41;
    const VK_SCROLL_LOCK: u32 = 0x91;
    const VK_SHIFT: u32 = 0x10;

    #[test]
    fn with_suppression_off_no_keyboard_event_is_withheld() {
        let mut gate = SuppressionGate::default();
        assert!(!gate.on_keyboard(false, WM_KEYDOWN, VK_A));
        assert!(!gate.on_keyboard(false, WM_KEYUP, VK_A));
        assert!(!gate.on_keyboard(false, WM_SYSKEYDOWN, VK_A));
        assert!(!gate.on_keyboard(false, WM_SYSKEYUP, VK_A));
    }

    #[test]
    fn with_suppression_on_a_full_press_and_release_are_both_withheld() {
        let mut gate = SuppressionGate::default();
        assert!(gate.on_keyboard(true, WM_KEYDOWN, VK_A));
        assert!(gate.on_keyboard(true, WM_KEYUP, VK_A));
    }

    #[test]
    fn a_syskey_press_and_release_are_treated_like_a_plain_one() {
        let mut gate = SuppressionGate::default();
        assert!(gate.on_keyboard(true, WM_SYSKEYDOWN, VK_A));
        assert!(gate.on_keyboard(true, WM_SYSKEYUP, VK_A));
    }

    #[test]
    fn a_release_is_withheld_after_suppression_is_handed_back_if_its_press_was_withheld() {
        let mut gate = SuppressionGate::default();
        // Pressed while forwarding to the peer...
        assert!(gate.on_keyboard(true, WM_KEYDOWN, VK_A));
        // ...released after the route flipped back to local: still
        // withheld, so the local app never sees a dangling key-down.
        assert!(gate.on_keyboard(false, WM_KEYUP, VK_A));
        // And exactly once.
        assert!(!gate.on_keyboard(false, WM_KEYUP, VK_A));
    }

    #[test]
    fn a_release_reaches_the_local_app_when_its_press_did_even_if_suppression_turned_on_since() {
        let mut gate = SuppressionGate::default();
        // Pressed while local — the local app saw the key-down.
        assert!(!gate.on_keyboard(false, WM_KEYDOWN, VK_SHIFT));
        // Suppression turns on mid-hold; the release must still reach the
        // local app or its key stays stuck down there.
        assert!(!gate.on_keyboard(true, WM_KEYUP, VK_SHIFT));
    }

    #[test]
    fn the_switch_key_own_press_that_completes_before_suppression_is_a_clean_local_press() {
        // Windows→Mac: Scroll Lock goes down while Windows is still the
        // active device (suppression still off, so the local OS and the
        // switch-key matcher both see it), the daemon then switches and
        // turns suppression on, and the key-up arrives suppressed.
        let mut gate = SuppressionGate::default();
        assert!(!gate.on_keyboard(false, WM_KEYDOWN, VK_SCROLL_LOCK));
        // Up arrives after suppression turned on, but its down reached
        // local, so it does too — a complete, consistent Scroll Lock
        // press locally rather than a half one.
        assert!(!gate.on_keyboard(true, WM_KEYUP, VK_SCROLL_LOCK));
    }

    #[test]
    fn with_suppression_on_mouse_movement_and_wheel_are_withheld() {
        use windows::Win32::UI::WindowsAndMessaging::{
            WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
        };
        let mut gate = SuppressionGate::default();
        assert!(gate.on_mouse(true, WM_MOUSEMOVE));
        assert!(gate.on_mouse(true, WM_MOUSEWHEEL));
        assert!(gate.on_mouse(true, WM_MOUSEHWHEEL));
    }

    #[test]
    fn with_suppression_off_mouse_movement_and_wheel_pass_through() {
        use windows::Win32::UI::WindowsAndMessaging::{
            WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
        };
        let mut gate = SuppressionGate::default();
        assert!(!gate.on_mouse(false, WM_MOUSEMOVE));
        assert!(!gate.on_mouse(false, WM_MOUSEWHEEL));
        assert!(!gate.on_mouse(false, WM_MOUSEHWHEEL));
    }

    #[test]
    fn a_mouse_button_release_is_withheld_iff_its_press_was() {
        let mut gate = SuppressionGate::default();
        // Pressed while forwarding, released after the route flipped back.
        assert!(gate.on_mouse(true, WM_LBUTTONDOWN));
        assert!(gate.on_mouse(false, WM_LBUTTONUP));
        assert!(!gate.on_mouse(false, WM_LBUTTONUP));

        // Pressed while local, suppression turns on mid-hold: the release
        // still reaches the local app.
        assert!(!gate.on_mouse(false, WM_RBUTTONDOWN));
        assert!(!gate.on_mouse(true, WM_RBUTTONUP));
    }

    #[test]
    fn withheld_buttons_are_tracked_independently_per_button() {
        let mut gate = SuppressionGate::default();
        assert!(gate.on_mouse(true, WM_LBUTTONDOWN));
        assert!(gate.on_mouse(true, WM_RBUTTONDOWN));
        // Releasing left after the flag cleared: withheld (its down was);
        // right is still independently tracked.
        assert!(gate.on_mouse(false, WM_LBUTTONUP));
        assert!(gate.on_mouse(false, WM_RBUTTONUP));
        assert!(!gate.on_mouse(false, WM_RBUTTONUP));
    }
}
