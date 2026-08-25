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

use std::cell::RefCell;
use std::fmt;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use flow_core::input::InputCapture;
use flow_core::protocol::InputEvent;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_QUIT,
};

use super::translate::EventTranslator;

thread_local! {
    static STATE: RefCell<Option<CaptureState>> = const { RefCell::new(None) };
}

struct CaptureState {
    translator: EventTranslator,
    sender: Sender<InputEvent>,
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
}

impl WindowsInputCapture {
    pub fn new(sender: Sender<InputEvent>) -> Self {
        Self {
            sender,
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

        let sender = self.sender.clone();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, WindowsCaptureError>>();
        let worker = thread::spawn(move || run_capture_loop(sender, ready_tx));

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
}

fn run_capture_loop(sender: Sender<InputEvent>, ready: Sender<Result<u32, WindowsCaptureError>>) {
    STATE.with(|state| {
        *state.borrow_mut() = Some(CaptureState {
            translator: EventTranslator::new(),
            sender,
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
        let timestamp_ms = now_ms();
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                if let Some(event) =
                    state
                        .translator
                        .translate_keyboard(wparam.0 as u32, info, timestamp_ms)
                {
                    let _ = state.sender.send(event);
                }
            }
        });
    }
    // SAFETY: forwards to the next hook in the chain, as required by the
    // WH_KEYBOARD_LL contract regardless of whether this handled the event.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: for code >= 0, lparam points to a valid MSLLHOOKSTRUCT
        // for the duration of this call, per WH_MOUSE_LL's contract.
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let timestamp_ms = now_ms();
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                if let Some(event) =
                    state
                        .translator
                        .translate_mouse(wparam.0 as u32, info, timestamp_ms)
                {
                    let _ = state.sender.send(event);
                }
            }
        });
    }
    // SAFETY: forwards to the next hook in the chain, as required by the
    // WH_MOUSE_LL contract regardless of whether this handled the event.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}
