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

use crate::pairing::{PairingDecision, PairingRequest};
use crate::protocol::InputEvent;

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

/// Everything that can travel over a [`Channel`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChannelMessage {
    Input(InputEvent),
    Pairing(PairingWireMessage),
    Heartbeat,
    /// Raw bytes carried by `NoiseChannel` (`daemon/todos.json` H3):
    /// handshake material before its wrapped transport is established,
    /// or an encrypted, serialized `ChannelMessage` afterward. Never
    /// constructed or matched outside `daemon::channel::noise` — every
    /// other module only ever sees the decrypted `ChannelMessage`
    /// `NoiseChannel`'s own `Channel` implementation yields.
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
    /// didn't verify (`daemon/todos.json` H3) — covers both "the bytes
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
        a.send(ChannelMessage::Input(event.clone()))
            .await
            .expect("send");
        let received = b.recv().await.expect("recv");
        assert_eq!(received, ChannelMessage::Input(event));
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
