//! The Channel abstraction (`docs/architecture/channels.md`): a single
//! custom interface for daemon-to-daemon connectivity, backed by TCP or
//! Bluetooth depending on what's available between two devices. Nothing
//! in this module names a concrete medium — `flow-core`, and everything
//! that depends on it, only ever talks to [`Channel`].
//!
//! Replaces `core::transport::Transport`, a placeholder never
//! implemented anywhere: Channels additionally carry pairing handshake
//! messages and heartbeats over the same connection, not just input
//! events, per `docs/architecture/channels.md`'s wire shape.

use std::fmt;
use std::net::SocketAddr;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::control::Role;
use crate::device::{HostOs, ScreenSize};
use crate::pairing::{PairingDecision, PairingRequest};
use crate::protocol::{InputEvent, Modifier};

/// Which medium backs a [`Channel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Tcp,
    Bluetooth,
}

/// A Bluetooth device address, kept as a thin newtype rather than
/// re-exporting whichever Bluetooth crate's own address type G4 ends up
/// using — `ChannelAddress` (and everything above it) stays independent
/// of that choice.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BluetoothAddr(pub String);

impl fmt::Display for BluetoothAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Where to reach a peer, per medium.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelAddress {
    Tcp(SocketAddr),
    Bluetooth(BluetoothAddr),
}

/// A pairing handshake message traveling over a [`Channel`] — the same
/// connection input events use, not a separate one. Wraps
/// `core::pairing`'s existing request/decision types, defined ahead of
/// this track specifically for this purpose (see that module's own doc
/// comment).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PairingWireMessage {
    Request(PairingRequest),
    Decision(PairingDecision),
}

/// The daemon-to-daemon wire protocol version. A peer announcing another
/// version (or none: v1 peers speak a different message set) is refused.
pub const PROTOCOL_VERSION: u32 = 2;

/// Everything that can travel over a [`Channel`]
/// (`docs/adr/0001-controller-controlled-input-model.md`).
///
/// `seq` is assigned by the controller, one counter per connection shared
/// by `Enter`, `Leave` and `Input`, and the controlled side drops anything
/// at or below the last it accepted. A counter, not a timestamp: two
/// legitimate mouse deltas can share a millisecond.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChannelMessage {
    /// First frame from both sides: capabilities and role.
    Hello {
        protocol_version: u32,
        role: Role,
        os: HostOs,
        screen: ScreenSize,
        device_name: String,
    },
    /// controller -> controlled: you are now the input target; warp the
    /// cursor to `(x, y)`.
    Enter {
        seq: u64,
        x: i32,
        y: i32,
        held_modifiers: Vec<Modifier>,
    },
    /// controller -> controlled: stop being the target; release every
    /// held key and button.
    Leave {
        seq: u64,
    },
    /// controller -> controlled; only valid between `Enter` and `Leave`.
    Input {
        seq: u64,
        event: InputEvent,
    },
    /// controlled -> controller: the screen resolution changed.
    ScreenInfo {
        width: u32,
        height: u32,
    },
    /// controlled -> controller: the user wants their machine back.
    ReleaseRequest,
    Heartbeat,
    /// Orderly close, with a machine-readable reason (`role_conflict`,
    /// `protocol_mismatch`, `shutdown`, ...).
    Goodbye {
        reason: String,
    },
    Pairing(PairingWireMessage),
    /// Raw bytes for session establishment, carried by `NoiseChannel` for
    /// its handshake and (encrypted) for every serialized
    /// `ChannelMessage` afterwards, and by the dev-only plaintext
    /// handshake. Never constructed or matched outside those modules.
    Noise(Vec<u8>),
}

/// What can go wrong on a [`Channel`], independent of which medium is
/// underneath.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ChannelError {
    #[error("connection lost")]
    ConnectionLost,
    #[error("peer unreachable")]
    Unreachable,
    #[error("serialization failed: {0}")]
    Serialization(String),
    #[error("medium not supported on this platform")]
    UnsupportedMedium,
    /// The Noise handshake failed, or the peer's identity proof over it
    /// didn't verify (`docs/tasks/daemon-todos.json` H3) — covers both "the bytes
    /// were corrupt/tampered with" and "the signature didn't match,"
    /// deliberately not distinguished further since Noise itself
    /// doesn't distinguish a malformed handshake from a tampered one.
    #[error("peer identity verification failed")]
    AuthenticationFailed,
}

/// A connection between two Flow daemons, established over whichever
/// medium is actually available. `docs/architecture/channels.md`'s
/// central abstraction: pairing, input streaming, and encryption
/// (tracks G7, G8, H3) are written once against this trait and never
/// depend on `TcpChannel`/`BluetoothChannel` directly — only G6's
/// negotiation step (`connect_best_available`) knows both concrete
/// types exist.
#[async_trait]
pub trait Channel: Send {
    fn kind(&self) -> ChannelKind;
    async fn send(&mut self, msg: ChannelMessage) -> Result<(), ChannelError>;
    async fn recv(&mut self) -> Result<ChannelMessage, ChannelError>;
    async fn close(&mut self) -> Result<(), ChannelError>;
}

/// Lets a `Box<dyn Channel>` (what `negotiate::connect_best_available`
/// returns, since the caller doesn't know or care which concrete medium
/// it got) be wrapped by a decorator generic over `C: Channel` — e.g.
/// `NoiseChannel<Box<dyn Channel>>` — without that decorator needing its
/// own special case for "a boxed trait object" versus a concrete type.
#[async_trait]
impl Channel for Box<dyn Channel> {
    fn kind(&self) -> ChannelKind {
        (**self).kind()
    }

    async fn send(&mut self, msg: ChannelMessage) -> Result<(), ChannelError> {
        (**self).send(msg).await
    }

    async fn recv(&mut self) -> Result<ChannelMessage, ChannelError> {
        (**self).recv().await
    }

    async fn close(&mut self) -> Result<(), ChannelError> {
        (**self).close().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::KeyboardEvent;
    use tokio::sync::mpsc;

    /// An in-memory `Channel` pair connected by two `tokio::sync::mpsc`
    /// channels (one per direction) — proves `Channel` is exercisable
    /// without any real network or Bluetooth stack, per this task's own
    /// acceptance criteria.
    struct ChannelPair {
        tx: mpsc::UnboundedSender<ChannelMessage>,
        rx: mpsc::UnboundedReceiver<ChannelMessage>,
    }

    impl ChannelPair {
        fn new_pair() -> (Self, Self) {
            let (a_tx, b_rx) = mpsc::unbounded_channel();
            let (b_tx, a_rx) = mpsc::unbounded_channel();
            (Self { tx: a_tx, rx: a_rx }, Self { tx: b_tx, rx: b_rx })
        }
    }

    #[async_trait]
    impl Channel for ChannelPair {
        fn kind(&self) -> ChannelKind {
            ChannelKind::Tcp
        }

        async fn send(&mut self, msg: ChannelMessage) -> Result<(), ChannelError> {
            self.tx.send(msg).map_err(|_| ChannelError::ConnectionLost)
        }

        async fn recv(&mut self) -> Result<ChannelMessage, ChannelError> {
            self.rx.recv().await.ok_or(ChannelError::ConnectionLost)
        }

        async fn close(&mut self) -> Result<(), ChannelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_heartbeat_sent_on_one_end_is_received_on_the_other() {
        let (mut a, mut b) = ChannelPair::new_pair();
        a.send(ChannelMessage::Heartbeat).await.expect("send");
        let received = b.recv().await.expect("recv");
        assert_eq!(received, ChannelMessage::Heartbeat);
    }

    #[tokio::test]
    async fn an_input_event_round_trips() {
        let (mut a, mut b) = ChannelPair::new_pair();
        let event = InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: "A".to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        });
        let message = ChannelMessage::Input { seq: 1, event };
        a.send(message.clone()).await.expect("send");
        let received = b.recv().await.expect("recv");
        assert_eq!(received, message);
    }

    #[tokio::test]
    async fn a_hello_round_trips() {
        let (mut a, mut b) = ChannelPair::new_pair();
        let message = ChannelMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            role: Role::Controller,
            os: HostOs::Windows,
            screen: ScreenSize {
                width: 1920,
                height: 1080,
            },
            device_name: "Desk".to_string(),
        };
        a.send(message.clone()).await.expect("send");
        assert_eq!(b.recv().await.expect("recv"), message);
    }

    #[tokio::test]
    async fn a_pairing_request_round_trips() {
        let (mut a, mut b) = ChannelPair::new_pair();
        let request = PairingRequest {
            device_name: "MacBook".to_string(),
            device_os: crate::device::HostOs::Macos,
            address: "192.168.1.42:47900".to_string(),
        };
        a.send(ChannelMessage::Pairing(PairingWireMessage::Request(
            request.clone(),
        )))
        .await
        .expect("send");
        let received = b.recv().await.expect("recv");
        assert_eq!(
            received,
            ChannelMessage::Pairing(PairingWireMessage::Request(request))
        );
    }

    #[tokio::test]
    async fn recv_on_a_closed_sender_reports_connection_lost() {
        let (a, mut b) = ChannelPair::new_pair();
        drop(a);
        assert_eq!(b.recv().await, Err(ChannelError::ConnectionLost));
    }

    #[tokio::test]
    async fn each_channel_reports_its_own_kind() {
        let (a, _b) = ChannelPair::new_pair();
        assert_eq!(a.kind(), ChannelKind::Tcp);
    }
}
