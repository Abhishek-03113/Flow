//! Spawns the hotkey runner (`daemon/todos.json` F2): bridges the
//! platform's real input-capture stream through [`super::SwitchKeyMatcher`],
//! calling [`DaemonService::switch_active_device_local`] on a match —
//! independent of any IPC client, per `vision.md` §8 ("Daemon Works
//! without UI... the switch interaction must not require the UI to be
//! running").

use std::sync::mpsc;
use std::thread;

use flow_core::input::InputCapture;
use flow_platform::DefaultInputCapture;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::task::JoinHandle;

use super::SwitchKeyMatcher;
use crate::service::DaemonService;

/// Starts the platform's real input capture and spawns the task that
/// feeds it through a [`SwitchKeyMatcher`].
///
/// Returns `None` (logging a warning, not panicking) when the platform
/// adapter can't start — no capturable device found, missing
/// permission, and so on. The daemon still serves IPC normally without
/// it: the hotkey is one input path among others, not a startup
/// dependency. This matters concretely in this project's own
/// development container, which has neither `/dev/input` nor
/// `/dev/uinput` (`daemon/README.md`'s "Platform adapters" table) and
/// must still be able to run `flow-daemon` for every other manual/
/// automated verification recipe already documented there.
pub fn spawn(service: &DaemonService) -> Option<JoinHandle<()>> {
    let (sender, receiver) = mpsc::channel();
    let mut capture = DefaultInputCapture::new(sender);
    if let Err(err) = capture.start() {
        tracing::warn!("hotkey runner not started: input capture failed: {err:?}");
        return None;
    }

    // Bridges the capture thread's std::sync::mpsc onto a channel this
    // async task can await, since InputCapture's contract (shared by
    // every platform adapter) is synchronous.
    let (bridge_tx, mut bridge_rx) = tokio_mpsc::unbounded_channel();
    thread::spawn(move || {
        for event in receiver {
            if bridge_tx.send(event).is_err() {
                break;
            }
        }
    });

    let service = service.clone();
    let mut settings_rx = service.watch_settings();
    let mut matcher = SwitchKeyMatcher::new(settings_rx.borrow_and_update().switch_key.clone());

    Some(tokio::spawn(async move {
        // Keeps the capture handle (and its OS-level hook/tap/thread)
        // alive for the runner's lifetime; dropping it early wouldn't
        // stop the underlying capture, but would lose the ability to
        // ever call `stop()` on it.
        let _capture = capture;
        loop {
            tokio::select! {
                event = bridge_rx.recv() => {
                    let Some(event) = event else { break; };
                    if matcher.feed(&event) {
                        service.switch_active_device_local().await;
                    }
                }
                changed = settings_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let binding = settings_rx.borrow_and_update().switch_key.clone();
                    matcher.set_binding(binding);
                }
            }
        }
    }))
}
