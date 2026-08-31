//! Spawns the hotkey runner (`daemon/todos.json` F2): bridges the
//! platform's real input-capture stream through [`super::SwitchKeyMatcher`],
//! calling [`DaemonService::switch_active_device_local`] on a match —
//! independent of any IPC client, per `vision.md` §8 ("Daemon Works
//! without UI... the switch interaction must not require the UI to be
//! running").

use std::collections::HashSet;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use flow_core::input::InputCapture;
use flow_core::protocol::{InputEvent, KeyboardEvent};
use flow_platform::DefaultInputCapture;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::task::JoinHandle;

use super::debounce::{SwitchDebouncer, SWITCH_DEBOUNCE_WINDOW};
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
    let mut debouncer = SwitchDebouncer::new(SWITCH_DEBOUNCE_WINDOW);

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
                    // Key-repeat (holding the switch key) or a noisy
                    // multi-key combo release can make the matcher fire
                    // more than once for what a person experiences as a
                    // single press — the debouncer, not the matcher,
                    // collapses that into one actual switch (F3).
                    if matcher.feed(&event) {
                        if service.peer_pipeline_active() {
                            // A peer pipeline owns switch-key authority
                            // while it runs — its own capture stream sees
                            // this key even while local suppression
                            // withholds it from the OS, and it handles
                            // the switch there. Standing down here is
                            // what stops the press switching twice.
                            continue;
                        }
                        if debouncer.should_fire(Instant::now()) {
                            service.switch_active_device_local().await;
                        }
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

/// Feeds an already-running capture stream — a peer pipeline's own, which
/// keeps producing events even while local suppression withholds them
/// from the OS — through the switch-key matcher, and returns that same
/// stream minus the events that completed the switch binding. The switch
/// key therefore triggers a local switch (via
/// [`DaemonService::switch_active_device_local`]) without ever being
/// forwarded to the remote machine.
///
/// `main.rs`'s `run_peer_pipeline` uses this in place of [`spawn`] for the
/// duration of a connection: once the pipeline's hook starts returning
/// `LRESULT(1)` the standalone [`spawn`] runner's separate hook no longer
/// sees the switch key at all (`daemon/README.md`, "Local input
/// suppression"; `todos-fix-physical-input-switching.md` §5), so
/// detection has to move onto the stream that is still live. The
/// standalone runner stands down meanwhile — see
/// [`DaemonService::peer_pipeline_active`].
pub fn spawn_pipeline_switch_filter(
    service: DaemonService,
    mut input: tokio_mpsc::UnboundedReceiver<InputEvent>,
) -> tokio_mpsc::UnboundedReceiver<InputEvent> {
    let (out_tx, out_rx) = tokio_mpsc::unbounded_channel();
    let mut settings_rx = service.watch_settings();
    let mut matcher = SwitchKeyMatcher::new(settings_rx.borrow_and_update().switch_key.clone());
    let mut debouncer = SwitchDebouncer::new(SWITCH_DEBOUNCE_WINDOW);
    // Key names whose `KeyDown` this filter consumed as a switch trigger,
    // so the matching `KeyUp` is consumed too rather than leaking to the
    // peer as an orphan release.
    let mut consumed_keys: HashSet<String> = HashSet::new();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                event = input.recv() => {
                    let Some(event) = event else { break; };
                    // Always feed the matcher, even for an event about to
                    // be consumed, so its held-key state stays accurate.
                    let matched = matcher.feed(&event);
                    if matched && debouncer.should_fire(Instant::now()) {
                        if let InputEvent::Keyboard(KeyboardEvent::KeyDown { key, .. }) = &event {
                            consumed_keys.insert(key.clone());
                        }
                        crate::hop_note!(
                            stage = "switch_consumed",
                            role = "owner",
                            trigger = "hotkey",
                            "switch key matched inside a peer pipeline; not forwarding it to the peer"
                        );
                        service.switch_active_device_local().await;
                        continue;
                    }
                    if let InputEvent::Keyboard(KeyboardEvent::KeyUp { key, .. }) = &event {
                        if consumed_keys.remove(key) {
                            continue;
                        }
                    }
                    if out_tx.send(event).is_err() {
                        break;
                    }
                }
                changed = settings_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    matcher.set_binding(settings_rx.borrow_and_update().switch_key.clone());
                    consumed_keys.clear();
                }
            }
        }
    });

    out_rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use flow_core::protocol::InputEvent;

    async fn test_service() -> DaemonService {
        let storage = Storage::open_in_memory().await.expect("in-memory db");
        DaemonService::new_seeded_for_test(storage).await
    }

    fn scroll_lock_down() -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: "ScrollLock".to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        })
    }

    fn scroll_lock_up() -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyUp {
            key: "ScrollLock".to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        })
    }

    fn letter_a_down() -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: "A".to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        })
    }

    fn active_device_id(devices: &[flow_core::device::Device]) -> Option<String> {
        devices
            .iter()
            .find(|d| d.state == flow_core::device::DeviceState::Active)
            .map(|d| d.id.0.clone())
    }

    #[tokio::test]
    async fn a_non_switch_event_passes_straight_through() {
        let service = test_service().await;
        let (tx, rx) = tokio_mpsc::unbounded_channel();
        let mut out = spawn_pipeline_switch_filter(service, rx);

        tx.send(letter_a_down()).unwrap();
        assert_eq!(out.recv().await, Some(letter_a_down()));
    }

    #[tokio::test]
    async fn the_switch_key_is_consumed_not_forwarded_and_triggers_a_switch() {
        let service = test_service().await;
        let mut devices_rx = service.watch_devices();
        let before = active_device_id(&devices_rx.borrow_and_update()).expect("an active device");

        let (tx, rx) = tokio_mpsc::unbounded_channel();
        let mut out = spawn_pipeline_switch_filter(service, rx);

        tx.send(scroll_lock_down()).unwrap();
        tx.send(scroll_lock_up()).unwrap();
        // A normal event afterwards is what we actually receive — proving
        // both Scroll Lock events were withheld, not merely delayed.
        tx.send(letter_a_down()).unwrap();
        assert_eq!(out.recv().await, Some(letter_a_down()));

        devices_rx.changed().await.expect("devices updated");
        let after = active_device_id(&devices_rx.borrow_and_update()).expect("an active device");
        assert_ne!(before, after, "the active device should have switched");
    }

    #[tokio::test]
    async fn a_rebind_makes_the_old_switch_key_forward_normally_again() {
        let service = test_service().await;
        // presets()[2] is F13.
        service
            .update_settings(flow_core::settings::SettingsPatch {
                switch_key: Some(flow_core::switch_key::presets()[2].clone()),
                ..Default::default()
            })
            .await
            .expect("rebind");

        let (tx, rx) = tokio_mpsc::unbounded_channel();
        let mut out = spawn_pipeline_switch_filter(service, rx);
        // Give the filter task a moment to observe the settings value it
        // reads at startup (it is the post-rebind one here anyway).
        tx.send(scroll_lock_down()).unwrap();
        assert_eq!(out.recv().await, Some(scroll_lock_down()));
    }
}
