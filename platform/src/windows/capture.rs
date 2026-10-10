//! [`WindowsInputCapture`]: installs `WH_KEYBOARD_LL`/`WH_MOUSE_LL` hooks
//! and hands each translated event to the [`CaptureSink`].
//!
//! Low-level hooks require a message loop pumping on the thread that
//! installed them, so `start()` spawns a dedicated thread for both hooks
//! and the loop. Hook procedures are plain `extern "system"` functions with
//! no user data, so the translator and sink live in thread-local storage,
//! populated before the hooks are installed — safe because Windows always
//! calls a hook procedure on the thread that registered it.
//!
//! Whether an event reaches local applications is decided synchronously,
//! inside the hook, by the sink's [`Disposition`]: a swallowed event
//! returns `LRESULT(1)` instead of chaining to `CallNextHookEx`.

use std::cell::RefCell;
use std::fmt;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use flow_core::input::{CaptureSink, Disposition, InputCapture};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, GetSystemMetrics, PostThreadMessageW,
    SetCursorPos, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, SM_CXSCREEN, SM_CYSCREEN, WH_KEYBOARD_LL,
    WH_MOUSE_LL, WM_MOUSEMOVE, WM_QUIT,
};

use super::translate::EventTranslator;
use super::FLOW_INJECTED_MARKER;

thread_local! {
    static STATE: RefCell<Option<CaptureState>> = const { RefCell::new(None) };
}

struct CaptureState {
    translator: EventTranslator,
    sink: Arc<dyn CaptureSink>,
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

/// Captures keyboard/mouse input via low-level hooks.
pub struct WindowsInputCapture {
    sink: Arc<dyn CaptureSink>,
    capture_thread_id: Option<u32>,
    worker: Option<JoinHandle<()>>,
}

impl WindowsInputCapture {
    pub fn new(sink: Arc<dyn CaptureSink>) -> Self {
        Self {
            sink,
            capture_thread_id: None,
            worker: None,
        }
    }
}

impl InputCapture for WindowsInputCapture {
    type Error = WindowsCaptureError;

    fn start(&mut self) -> Result<(), Self::Error> {
        if self.worker.is_some() {
            return Ok(());
        }

        let sink = Arc::clone(&self.sink);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, WindowsCaptureError>>();
        let worker = thread::spawn(move || run_capture_loop(sink, ready_tx));

        match ready_rx.recv() {
            Ok(Ok(thread_id)) => {
                self.capture_thread_id = Some(thread_id);
                self.worker = Some(worker);
                Ok(())
            }
            Ok(Err(err)) => Err(err),
            // The thread exited before reporting success.
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
}

fn run_capture_loop(sink: Arc<dyn CaptureSink>, ready: Sender<Result<u32, WindowsCaptureError>>) {
    STATE.with(|state| {
        *state.borrow_mut() = Some(CaptureState {
            translator: EventTranslator::new(),
            sink,
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
        // Synthetic input — this daemon's own, or another program's — is
        // not physical input and must never be forwarded or hidden.
        let injected =
            info.dwExtraInfo == FLOW_INJECTED_MARKER || info.flags.0 & LLKHF_INJECTED.0 != 0;
        if !injected {
            let message = wparam.0 as u32;
            let timestamp_ms = now_ms();
            let swallow = guard_hook_body("keyboard", || {
                STATE.with(|state| {
                    let mut slot = state.borrow_mut();
                    let Some(state) = slot.as_mut() else {
                        return false;
                    };
                    match state
                        .translator
                        .translate_keyboard(message, info, timestamp_ms)
                    {
                        Some(event) => state.sink.on_event(&event) == Disposition::Swallow,
                        None => state.sink.suppressing(),
                    }
                })
            });
            if swallow {
                return LRESULT(1);
            }
        }
    }
    // SAFETY: forwards to the next hook in the chain, as required by the
    // WH_KEYBOARD_LL contract when this callback did not swallow the event.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: for code >= 0, lparam points to a valid MSLLHOOKSTRUCT
        // for the duration of this call, per WH_MOUSE_LL's contract.
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        // Skips synthetic input, which includes the `SetCursorPos`
        // recenter below echoing back through this hook.
        let injected = info.dwExtraInfo == FLOW_INJECTED_MARKER || info.flags & LLMHF_INJECTED != 0;
        if !injected {
            let message = wparam.0 as u32;
            let timestamp_ms = now_ms();
            let anchor = screen_center();
            let swallow = guard_hook_body("mouse", || {
                STATE.with(|state| {
                    let mut slot = state.borrow_mut();
                    let Some(state) = slot.as_mut() else {
                        return false;
                    };
                    let swallow =
                        match state
                            .translator
                            .translate_mouse(message, info, timestamp_ms)
                        {
                            Some(event) => state.sink.on_event(&event) == Disposition::Swallow,
                            None => state.sink.suppressing(),
                        };
                    if swallow && message == WM_MOUSEMOVE {
                        state.translator.reset_move_anchor(anchor.0, anchor.1);
                    }
                    swallow
                })
            });
            if swallow {
                if message == WM_MOUSEMOVE {
                    // Recenter the real (hidden-from-apps) cursor — see
                    // `screen_center` for why swallowing alone isn't enough
                    // to keep relative motion meaningful.
                    // SAFETY: SetCursorPos takes plain screen coordinates.
                    let _ = unsafe { SetCursorPos(anchor.0, anchor.1) };
                }
                return LRESULT(1);
            }
        }
    }
    // SAFETY: forwards to the next hook in the chain, as required by the
    // WH_MOUSE_LL contract when this callback did not swallow the event.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Runs one hook callback's translate-and-decide body, catching any
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
/// Returns whether the event should be swallowed. On a
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

/// The primary display's center point, in screen coordinates — the fixed
/// anchor [`mouse_proc`] warps the real cursor back to (`SetCursorPos`)
/// every time it withholds a `WM_MOUSEMOVE` from local delivery.
///
/// Withholding alone (`LRESULT(1)`, never chaining to `CallNextHookEx`)
/// does not keep `MSLLHOOKSTRUCT.pt` meaningful: verified empirically
/// (`WH_MOUSE_LL` always returning 1 for `WM_MOUSEMOVE`, driven with
/// synthetic `SendInput` motion far larger than one screen), `pt` freezes
/// at wherever the real, on-screen cursor was standing when withholding
/// began and only wobbles by a stray pixel of noise afterward — nowhere
/// close to a genuine display edge — no matter how much further motion
/// keeps arriving. Windows never advances the position it hands back to
/// the hook once nothing actually moves the visible cursor. Diffing that
/// frozen `pt` (`translate::EventTranslator::translate_move`) collapses
/// every subsequent move to a near-zero delta, which is why forwarded
/// mouse movement dies completely while keyboard forwarding (carrying no
/// positional state) keeps working.
///
/// Warping the real cursor back to a known point after each withheld move
/// — the same fix `SetCursorPos`-based KVM tools (Barrier/Synergy/
/// Deskflow) apply on their capturing side — keeps genuine relative
/// motion flowing: each move is measured from the anchor, not from an
/// ever-more-stale frozen position, so it can never approach a real edge.
/// [`translate::EventTranslator::reset_move_anchor`] tells the translator
/// to expect that jump rather than diff against it. Confirmed against
/// the same synthetic-drive harness: every subsequent move reports its
/// true delta instead of collapsing to noise.
///
/// A side effect shared with those same reference implementations: the
/// local cursor is left sitting at this anchor point once suppression
/// ends, not wherever the physical mouse actually is — a documented,
/// accepted trade-off of the technique, not something this fix attempts
/// to hide.
fn screen_center() -> (i32, i32) {
    // SAFETY: GetSystemMetrics has no preconditions; SM_CXSCREEN/
    // SM_CYSCREEN are always valid indices.
    let (width, height) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    (width / 2, height / 2)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}
