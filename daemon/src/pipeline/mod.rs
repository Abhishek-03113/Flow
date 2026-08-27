//! The end-to-end input streaming pipeline (`daemon/todos.json` G8):
//! capture (E1) -> switch-aware gate (only while this device is
//! `Active`, per F2/F3's local switch state) -> `Channel::send` on the
//! sending side; `Channel::recv` -> injector (E2) on the receiving
//! side. Coded entirely against `flow_core::channel::Channel` and
//! `flow_core::input::{InputCapture, InputInjector}` — never a concrete
//! medium or platform type — so the gating logic here is exactly what
//! this module's own tests exercise without real hardware or a real
//! network (a real, loopback-connected `TcpChannel` stands in for "any
//! `Channel`", the same substitution `channel::negotiate`/`::handshake`'s
//! own tests already make).
//!
//! `vision.md`'s North Star ("just press a key and continue working")
//! is this pipeline: it's the first place capture, a `Channel`, and
//! injection are wired into one continuous loop.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use flow_core::channel::{Channel, ChannelMessage};
use flow_core::device::{Device, DeviceId, DeviceState};
use flow_core::input::InputInjector;
use flow_core::protocol::{InputEvent, KeyboardEvent, MouseButton, MouseEvent};
use tokio::sync::{mpsc, watch};

use crate::service::LOCAL_DEVICE_ID;

/// Whether the local device (`LOCAL_DEVICE_ID`) is currently `Active` —
/// the same eligibility flag `devices_list`'s callers already read off
/// `DaemonService::watch_devices()`, factored out as a pure function so
/// the gate below is unit-testable without spinning up a whole service.
fn is_local_device_active(devices: &[Device]) -> bool {
    devices
        .iter()
        .find(|device| device.id == DeviceId(LOCAL_DEVICE_ID.to_string()))
        .is_some_and(|device| device.state == DeviceState::Active)
}

/// The sending side: forwards every captured event onto `channel` as a
/// `ChannelMessage::Input`, tagged with a per-connection sequence number
/// (`daemon/todos.json` H4, revised — see `ChannelMessage::Input`'s own
/// doc comment for why this replaced a timestamp-based check), but only
/// while the local device is `Active` per `devices` — an event captured
/// while `Inactive` (this machine isn't the one currently "driving") is
/// silently dropped, not queued for later, and does *not* consume a
/// sequence number. Returns once `capture_events` closes (capture
/// stopped) or `channel.send` fails (peer gone).
pub async fn send_while_active(
    mut capture_events: mpsc::UnboundedReceiver<InputEvent>,
    mut devices: watch::Receiver<Vec<Device>>,
    mut channel: Box<dyn Channel>,
) {
    let mut sequence: u64 = 0;
    loop {
        tokio::select! {
            event = capture_events.recv() => {
                let Some(event) = event else { break; };
                if is_local_device_active(&devices.borrow_and_update()) {
                    sequence += 1;
                    if channel.send(ChannelMessage::Input { sequence, event }).await.is_err() {
                        break;
                    }
                }
            }
            changed = devices.changed() => {
                if changed.is_err() {
                    break;
                }
            }
        }
    }
}

/// The receiving side: injects every `ChannelMessage::Input` that
/// arrives over `channel`. Anything else on the same connection
/// (`Pairing`/`Heartbeat` traffic sharing it once G7's handshake and
/// this pipeline run concurrently) is ignored rather than treated as an
/// error. A single failed `inject` (e.g. a transient OS-level rejection)
/// doesn't end the loop — only the `Channel` closing does.
///
/// Replay protection (`daemon/todos.json` H4): each message's sender-
/// assigned `sequence` must strictly increase from the last *accepted*
/// message's — anything arriving with an equal or lower sequence is a
/// duplicate or replayed frame and is dropped, not injected. Deliberately
/// not derived from `event.timestamp_ms()`: two legitimate high-frequency
/// events (consecutive mouse-move deltas, say) can land on the same
/// millisecond under a coarse OS clock, which a timestamp-based check
/// can't distinguish from an actual replay without either wrongly
/// dropping real input or wrongly accepting a replay.
///
/// Stuck-input safety (daemon review gap #18): if the connection drops
/// between a `KeyDown`/`ButtonDown` this loop already injected and its
/// matching `KeyUp`/`ButtonUp`, the remote OS is left believing that
/// key/button is held forever — there's no third party to tell it
/// otherwise once the `Channel` that would have carried the release is
/// gone. `HeldInputTracker` exists specifically to make that release
/// happen anyway, synthesized locally, the moment this loop ends for any
/// reason.
pub async fn receive_and_inject<I>(mut channel: Box<dyn Channel>, mut injector: I)
where
    I: InputInjector,
    I::Error: std::fmt::Debug,
{
    let mut last_sequence: Option<u64> = None;
    let mut held = HeldInputTracker::default();
    loop {
        match channel.recv().await {
            Ok(ChannelMessage::Input { sequence, event }) => {
                if last_sequence.is_some_and(|last| sequence <= last) {
                    continue;
                }
                last_sequence = Some(sequence);
                match injector.inject(&event) {
                    Ok(()) => held.observe(&event),
                    Err(err) => tracing::warn!("input injection failed: {err:?}"),
                }
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    held.release_all(&mut injector);
}

/// Tracks which keys/mouse buttons this side has injected a `KeyDown`/
/// `ButtonDown` for with no matching `KeyUp`/`ButtonUp` seen yet, so
/// [`receive_and_inject`] can synthesize the release itself once the
/// `Channel` that would have carried a real one is gone — the mitigation
/// `daemon/todos.json`'s review calls a "hard invariant": a dropped
/// connection must never leave the remote OS believing input is
/// permanently held.
#[derive(Default)]
struct HeldInputTracker {
    keys: HashSet<String>,
    buttons: HashSet<MouseButton>,
}

impl HeldInputTracker {
    /// Updates held state from an event this side just successfully
    /// injected. Only ever called on a successful `inject` — an event the
    /// OS never actually saw shouldn't register as newly held.
    fn observe(&mut self, event: &InputEvent) {
        match event {
            InputEvent::Keyboard(KeyboardEvent::KeyDown { key, .. }) => {
                self.keys.insert(key.clone());
            }
            InputEvent::Keyboard(KeyboardEvent::KeyUp { key, .. }) => {
                self.keys.remove(key);
            }
            InputEvent::Mouse(MouseEvent::ButtonDown { button, .. }) => {
                self.buttons.insert(*button);
            }
            InputEvent::Mouse(MouseEvent::ButtonUp { button, .. }) => {
                self.buttons.remove(button);
            }
            InputEvent::Mouse(MouseEvent::Move { .. })
            | InputEvent::Mouse(MouseEvent::Scroll { .. }) => {}
        }
    }

    /// Synthesizes and injects a `KeyUp`/`ButtonUp` for everything still
    /// tracked as held, then clears. A failure releasing one held
    /// key/button doesn't stop the rest from being attempted — a partial
    /// release is still strictly better than releasing none.
    fn release_all<I>(&mut self, injector: &mut I)
    where
        I: InputInjector,
        I::Error: std::fmt::Debug,
    {
        let timestamp_ms = now_ms();
        for key in self.keys.drain() {
            let event = InputEvent::Keyboard(KeyboardEvent::KeyUp {
                key,
                modifiers: Vec::new(),
                timestamp_ms,
            });
            if let Err(err) = injector.inject(&event) {
                tracing::warn!("failed to release a held key on disconnect: {err:?}");
            }
        }
        for button in self.buttons.drain() {
            let event = InputEvent::Mouse(MouseEvent::ButtonUp {
                button,
                timestamp_ms,
            });
            if let Err(err) = injector.inject(&event) {
                tracing::warn!("failed to release a held mouse button on disconnect: {err:?}");
            }
        }
    }
}

/// The full-duplex counterpart to [`send_while_active`]/[`receive_and_inject`]
/// for a single already-authenticated connection to a paired peer, where
/// either side may become the locally-active device at different points
/// over that connection's lifetime — so this daemon must be able to both
/// send (while local) and receive-and-inject (while remote) over the
/// *same* connection, not two separate ones.
///
/// `Channel::send`/`::recv` both take `&mut self`, and nothing in this
/// codebase splits a `Channel` into independent read/write halves (see
/// `channel::noise::NoiseChannel`'s single shared `TransportState`, used
/// for both directions — splitting it safely would need real redesign,
/// not just here). So rather than run `send_while_active` and
/// `receive_and_inject` concurrently on two tasks sharing one channel,
/// this single task interleaves both directions itself with
/// `tokio::select!` — the same technique `send_while_active` already
/// uses to race its `capture_events`/`devices` inputs against each
/// other, just extended to a third branch reading off `channel` too.
pub async fn run_paired_connection<I>(
    mut channel: Box<dyn Channel>,
    mut capture_events: mpsc::UnboundedReceiver<InputEvent>,
    mut devices: watch::Receiver<Vec<Device>>,
    mut injector: I,
) where
    I: InputInjector,
    I::Error: std::fmt::Debug,
{
    let mut send_sequence: u64 = 0;
    let mut last_received_sequence: Option<u64> = None;
    let mut held = HeldInputTracker::default();

    loop {
        tokio::select! {
            event = capture_events.recv() => {
                let Some(event) = event else { break; };
                if is_local_device_active(&devices.borrow_and_update()) {
                    send_sequence += 1;
                    if channel.send(ChannelMessage::Input { sequence: send_sequence, event }).await.is_err() {
                        break;
                    }
                }
            }
            changed = devices.changed() => {
                if changed.is_err() {
                    break;
                }
            }
            received = channel.recv() => {
                match received {
                    Ok(ChannelMessage::Input { sequence, event }) => {
                        if last_received_sequence.is_some_and(|last| sequence <= last) {
                            continue;
                        }
                        last_received_sequence = Some(sequence);
                        match injector.inject(&event) {
                            Ok(()) => held.observe(&event),
                            Err(err) => tracing::warn!("input injection failed: {err:?}"),
                        }
                    }
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        }
    }
    held.release_all(&mut injector);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::tcp::TcpChannel;
    use flow_core::device::HostOs;
    use flow_core::protocol::{InputEvent, KeyboardEvent};
    use tokio::net::TcpListener;

    fn a_key_event(key: &str) -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: key.to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        })
    }

    fn local_device(state: DeviceState) -> Device {
        Device {
            id: DeviceId(LOCAL_DEVICE_ID.to_string()),
            name: "This Machine".to_string(),
            os: HostOs::Linux,
            state,
            last_seen: chrono::Utc::now(),
        }
    }

    async fn connected_pair() -> (Box<dyn Channel>, Box<dyn Channel>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            let (stream, _peer) = listener.accept().await.expect("accept");
            TcpChannel::accept(stream).await.expect("accept ws")
        });
        let client = TcpChannel::connect(addr).await.expect("connect");
        let server = server.await.expect("server task");
        (Box::new(client), Box::new(server))
    }

    #[test]
    fn the_local_device_is_reported_active_only_when_its_own_state_is_active() {
        assert!(is_local_device_active(&[local_device(DeviceState::Active)]));
        assert!(!is_local_device_active(&[local_device(
            DeviceState::Inactive
        )]));
        assert!(!is_local_device_active(&[]));
    }

    #[tokio::test]
    async fn an_event_captured_while_active_is_streamed_to_the_peer() {
        let (sender_side, mut receiver_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(vec![local_device(DeviceState::Active)]);
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();

        let pipeline = tokio::spawn(send_while_active(capture_rx, devices_rx, sender_side));

        let event = a_key_event("A");
        capture_tx.send(event.clone()).expect("send captured event");
        let received = receiver_side.recv().await.expect("recv");
        assert_eq!(received, ChannelMessage::Input { sequence: 1, event });

        drop(capture_tx);
        drop(devices_tx);
        pipeline.await.expect("pipeline task");
    }

    #[tokio::test]
    async fn an_event_captured_while_inactive_is_dropped_not_streamed() {
        let (sender_side, mut receiver_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(vec![local_device(DeviceState::Inactive)]);
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();

        let pipeline = tokio::spawn(send_while_active(capture_rx, devices_rx, sender_side));

        capture_tx
            .send(a_key_event("dropped"))
            .expect("send captured event while inactive");
        // Closing the capture channel (rather than racing a state flip
        // against an unsynchronized second send) is what makes this
        // deterministic: `send_while_active`'s loop drains every queued
        // capture event — applying the gate to each in turn — before its
        // `recv()` branch finally yields `None` and the loop, and the
        // `Channel` it owns, both drop.
        drop(capture_tx);
        drop(devices_tx);
        pipeline.await.expect("pipeline task");

        // If "dropped" had been sent despite being captured while
        // Inactive, it would arrive before this — proving the gate
        // actually suppressed it, not just that nothing happened to
        // race in yet.
        assert_eq!(
            receiver_side.recv().await,
            Err(flow_core::channel::ChannelError::ConnectionLost)
        );
    }

    /// A minimal `InputInjector` test double: records every event handed
    /// to it instead of touching real hardware. Uses an async channel
    /// (unbounded `send` is synchronous, so this still fits the
    /// synchronous `InputInjector::inject` signature) rather than
    /// `std::sync::mpsc` — a blocking `recv()` on that from within a
    /// `#[tokio::test]`'s single-threaded runtime would starve the very
    /// executor the spawned `receive_and_inject` task needs to run on,
    /// deadlocking the test.
    struct RecordingInjector {
        received: mpsc::UnboundedSender<InputEvent>,
    }

    impl InputInjector for RecordingInjector {
        type Error = std::convert::Infallible;

        fn inject(&mut self, event: &InputEvent) -> Result<(), Self::Error> {
            let _ = self.received.send(event.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_received_input_message_is_handed_to_the_injector() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        let event = a_key_event("Z");
        sender_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: event.clone(),
            })
            .await
            .expect("send");

        let injected = rx.recv().await.expect("injector received the event");
        assert_eq!(injected, event);

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");
    }

    #[tokio::test]
    async fn a_non_input_message_is_ignored_not_injected() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        sender_side
            .send(ChannelMessage::Heartbeat)
            .await
            .expect("send heartbeat");
        let event = a_key_event("after heartbeat");
        sender_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: event.clone(),
            })
            .await
            .expect("send input");

        let injected = rx.recv().await.expect("injector received the input event");
        assert_eq!(injected, event);
        assert!(rx.try_recv().is_err(), "the heartbeat must not be injected");

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");
    }

    #[tokio::test]
    async fn a_duplicate_sequence_frame_is_dropped_not_injected() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        // Mouse::Move deliberately, not a KeyDown: that isolates this
        // test to sequence-based replay dropping alone, with no
        // held-key release (a KeyDown never released before disconnect
        // would itself inject a synthesized KeyUp — a real and separately
        // tested behavior, see `a_key_held_when_the_connection_drops_is_released_not_left_stuck`,
        // just not what this test is about).
        let first = InputEvent::Mouse(MouseEvent::Move {
            dx: 1,
            dy: 1,
            timestamp_ms: 0,
        });
        sender_side
            .send(ChannelMessage::Input {
                sequence: 5,
                event: first.clone(),
            })
            .await
            .expect("send first");
        assert_eq!(rx.recv().await.expect("first event injected"), first);

        // Same sequence as the already-accepted message - a replayed
        // frame per H4's guard, must be dropped rather than injected,
        // even though its own timestamp_ms differs (proving the check is
        // on sequence, not timestamp).
        let replay = InputEvent::Mouse(MouseEvent::Move {
            dx: 99,
            dy: 99,
            timestamp_ms: 999,
        });
        sender_side
            .send(ChannelMessage::Input {
                sequence: 5,
                event: replay,
            })
            .await
            .expect("send replay");

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        assert!(
            rx.try_recv().is_err(),
            "the replayed frame must not have been injected"
        );
    }

    #[tokio::test]
    async fn an_out_of_order_lower_sequence_frame_is_dropped_not_injected() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        let first = a_key_event("first");
        sender_side
            .send(ChannelMessage::Input {
                sequence: 10,
                event: first.clone(),
            })
            .await
            .expect("send first");
        assert_eq!(rx.recv().await.expect("first event injected"), first);

        // Lower sequence than the last accepted message - dropped, not
        // injected, regardless of its own timestamp_ms.
        let stale = a_key_event("stale");
        sender_side
            .send(ChannelMessage::Input {
                sequence: 7,
                event: stale,
            })
            .await
            .expect("send stale");

        let next = a_key_event("next");
        sender_side
            .send(ChannelMessage::Input {
                sequence: 11,
                event: next.clone(),
            })
            .await
            .expect("send next");

        // The next value this channel yields is whatever was actually
        // injected - if the stale frame had slipped through, this would
        // be it instead of `next`, proving the drop deterministically
        // rather than by racing a timeout against nothing arriving.
        assert_eq!(rx.recv().await.expect("next event injected"), next);

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");
    }

    #[tokio::test]
    async fn send_while_active_assigns_strictly_increasing_sequence_numbers() {
        let (sender_side, mut receiver_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(vec![local_device(DeviceState::Active)]);
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();

        let pipeline = tokio::spawn(send_while_active(capture_rx, devices_rx, sender_side));

        capture_tx.send(a_key_event("one")).expect("send 1");
        capture_tx.send(a_key_event("two")).expect("send 2");
        capture_tx.send(a_key_event("three")).expect("send 3");

        // A single sender task processing an unbounded (FIFO) channel:
        // three sequential receives, in send order, are exactly what
        // arrives — no concurrency to coordinate here.
        let mut sequences = Vec::new();
        for _ in 0..3 {
            match receiver_side.recv().await.expect("recv") {
                ChannelMessage::Input { sequence, .. } => sequences.push(sequence),
                other => panic!("expected an Input message, got {other:?}"),
            }
        }
        assert_eq!(sequences, vec![1, 2, 3]);

        drop(capture_tx);
        drop(devices_tx);
        pipeline.await.expect("pipeline task");
    }

    fn key_down(key: &str) -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: key.to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        })
    }

    fn key_up(key: &str) -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyUp {
            key: key.to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        })
    }

    fn button_down(button: MouseButton) -> InputEvent {
        InputEvent::Mouse(MouseEvent::ButtonDown {
            button,
            timestamp_ms: 0,
        })
    }

    /// Reads whatever the injector received next, expecting a `KeyUp`/
    /// `ButtonUp` — the synthesized release, since nothing else in these
    /// tests injects one directly.
    async fn expect_next_is_key_up(rx: &mut mpsc::UnboundedReceiver<InputEvent>, key: &str) {
        match rx.recv().await.expect("a release event") {
            InputEvent::Keyboard(KeyboardEvent::KeyUp { key: released, .. }) => {
                assert_eq!(released, key);
            }
            other => panic!("expected a synthesized KeyUp for {key:?}, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_key_held_when_the_connection_drops_is_released_not_left_stuck() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        sender_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: key_down("A"),
            })
            .await
            .expect("send keydown");
        assert_eq!(rx.recv().await.expect("keydown injected"), key_down("A"));

        // The connection drops with no matching KeyUp ever sent — exactly
        // the "key_down sent, connection drops, key_up never arrives"
        // scenario the review calls out.
        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        expect_next_is_key_up(&mut rx, "A").await;
        assert!(
            rx.try_recv().is_err(),
            "exactly one synthesized release, nothing extra"
        );
    }

    #[tokio::test]
    async fn a_key_already_released_before_disconnect_is_not_released_again() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        sender_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: key_down("A"),
            })
            .await
            .expect("send keydown");
        assert_eq!(rx.recv().await.expect("keydown injected"), key_down("A"));

        sender_side
            .send(ChannelMessage::Input {
                sequence: 2,
                event: key_up("A"),
            })
            .await
            .expect("send keyup");
        assert_eq!(rx.recv().await.expect("keyup injected"), key_up("A"));

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        assert!(
            rx.try_recv().is_err(),
            "a key already released normally must not get a second, spurious release"
        );
    }

    #[tokio::test]
    async fn a_held_mouse_button_when_the_connection_drops_is_released() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        sender_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: button_down(MouseButton::Left),
            })
            .await
            .expect("send button down");
        assert_eq!(
            rx.recv().await.expect("button down injected"),
            button_down(MouseButton::Left)
        );

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        match rx.recv().await.expect("a release event") {
            InputEvent::Mouse(MouseEvent::ButtonUp { button, .. }) => {
                assert_eq!(button, MouseButton::Left);
            }
            other => panic!("expected a synthesized ButtonUp, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn multiple_held_keys_are_all_released_on_disconnect() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        for (sequence, key) in [(1, "Ctrl"), (2, "Shift"), (3, "A")] {
            sender_side
                .send(ChannelMessage::Input {
                    sequence,
                    event: key_down(key),
                })
                .await
                .expect("send keydown");
            assert_eq!(rx.recv().await.expect("keydown injected"), key_down(key));
        }

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        let mut released = std::collections::HashSet::new();
        for _ in 0..3 {
            match rx.recv().await.expect("a release event") {
                InputEvent::Keyboard(KeyboardEvent::KeyUp { key, .. }) => {
                    released.insert(key);
                }
                other => panic!("expected a synthesized KeyUp, got {other:?}"),
            }
        }
        assert_eq!(
            released,
            ["Ctrl", "Shift", "A"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
    }

    #[tokio::test]
    async fn run_paired_connection_handles_both_directions_over_one_connection() {
        let (mut peer_side, our_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(vec![local_device(DeviceState::Active)]);
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(run_paired_connection(
            our_side, capture_rx, devices_rx, injector,
        ));

        // Local side is active: a captured event streams out to the peer.
        let outgoing = a_key_event("A");
        capture_tx
            .send(outgoing.clone())
            .expect("send captured event");
        let received_by_peer = peer_side.recv().await.expect("recv");
        assert_eq!(
            received_by_peer,
            ChannelMessage::Input {
                sequence: 1,
                event: outgoing
            }
        );

        // The peer sends something back (as if it just became active
        // itself) — our side must inject it, over this same connection,
        // proving both directions share one `run_paired_connection` task
        // rather than needing two separate channels.
        let incoming = a_key_event("Z");
        peer_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: incoming.clone(),
            })
            .await
            .expect("send");
        let injected = rx.recv().await.expect("injector received the event");
        assert_eq!(injected, incoming);

        drop(capture_tx);
        drop(devices_tx);
        peer_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");
    }

    #[tokio::test]
    async fn run_paired_connection_releases_held_input_when_the_connection_drops() {
        let (mut peer_side, our_side) = connected_pair().await;
        let (_devices_tx, devices_rx) = watch::channel(vec![local_device(DeviceState::Inactive)]);
        let (_capture_tx, capture_rx) = mpsc::unbounded_channel();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(run_paired_connection(
            our_side, capture_rx, devices_rx, injector,
        ));

        peer_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: key_down("A"),
            })
            .await
            .expect("send keydown");
        assert_eq!(rx.recv().await.expect("keydown injected"), key_down("A"));

        peer_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        expect_next_is_key_up(&mut rx, "A").await;
    }
}
