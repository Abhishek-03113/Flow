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

use flow_core::channel::{Channel, ChannelMessage};
use flow_core::device::{Device, DeviceId, DeviceState};
use flow_core::input::InputInjector;
use flow_core::protocol::InputEvent;
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
/// `ChannelMessage::Input`, but only while the local device is `Active`
/// per `devices` — an event captured while `Inactive` (this machine
/// isn't the one currently "driving") is silently dropped, not queued
/// for later. Returns once `capture_events` closes (capture stopped) or
/// `channel.send` fails (peer gone).
pub async fn send_while_active(
    mut capture_events: mpsc::UnboundedReceiver<InputEvent>,
    mut devices: watch::Receiver<Vec<Device>>,
    mut channel: Box<dyn Channel>,
) {
    loop {
        tokio::select! {
            event = capture_events.recv() => {
                let Some(event) = event else { break; };
                if is_local_device_active(&devices.borrow_and_update())
                    && channel.send(ChannelMessage::Input(event)).await.is_err()
                {
                    break;
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
/// Replay protection (`daemon/todos.json` H4): each event's own
/// `timestamp_ms` (already carried by every `InputEvent` variant, so no
/// separate sequence field was added) must strictly increase from the
/// last *accepted* event's — anything arriving with an equal or lower
/// timestamp is a duplicate or replayed frame and is dropped, not
/// injected.
pub async fn receive_and_inject<I>(mut channel: Box<dyn Channel>, mut injector: I)
where
    I: InputInjector,
    I::Error: std::fmt::Debug,
{
    let mut last_timestamp_ms: Option<u64> = None;
    loop {
        match channel.recv().await {
            Ok(ChannelMessage::Input(event)) => {
                let timestamp_ms = event.timestamp_ms();
                if last_timestamp_ms.is_some_and(|last| timestamp_ms <= last) {
                    continue;
                }
                last_timestamp_ms = Some(timestamp_ms);
                if let Err(err) = injector.inject(&event) {
                    tracing::warn!("input injection failed: {err:?}");
                }
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::tcp::TcpChannel;
    use flow_core::device::HostOs;
    use flow_core::protocol::{InputEvent, KeyboardEvent};
    use tokio::net::TcpListener;

    fn a_key_event(key: &str) -> InputEvent {
        a_key_event_at(key, 0)
    }

    fn a_key_event_at(key: &str, timestamp_ms: u64) -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: key.to_string(),
            modifiers: vec![],
            timestamp_ms,
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
        assert_eq!(received, ChannelMessage::Input(event));

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
            .send(ChannelMessage::Input(event.clone()))
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
            .send(ChannelMessage::Input(event.clone()))
            .await
            .expect("send input");

        let injected = rx.recv().await.expect("injector received the input event");
        assert_eq!(injected, event);
        assert!(rx.try_recv().is_err(), "the heartbeat must not be injected");

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");
    }

    #[tokio::test]
    async fn a_duplicate_timestamp_frame_is_dropped_not_injected() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        let first = a_key_event_at("first", 100);
        sender_side
            .send(ChannelMessage::Input(first.clone()))
            .await
            .expect("send first");
        assert_eq!(rx.recv().await.expect("first event injected"), first);

        // Same timestamp_ms as the already-accepted event - a replayed
        // frame per H4's guard, must be dropped rather than injected.
        let replay = a_key_event_at("replay", 100);
        sender_side
            .send(ChannelMessage::Input(replay))
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
    async fn an_out_of_order_lower_timestamp_frame_is_dropped_not_injected() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        let first = a_key_event_at("first", 200);
        sender_side
            .send(ChannelMessage::Input(first.clone()))
            .await
            .expect("send first");
        assert_eq!(rx.recv().await.expect("first event injected"), first);

        // Older than the last accepted event - dropped, not injected.
        let stale = a_key_event_at("stale", 150);
        sender_side
            .send(ChannelMessage::Input(stale))
            .await
            .expect("send stale");

        let next = a_key_event_at("next", 250);
        sender_side
            .send(ChannelMessage::Input(next.clone()))
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
}
