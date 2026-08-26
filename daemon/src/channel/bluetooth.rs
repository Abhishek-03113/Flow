//! [`BluetoothChannel`]: the `Channel` implementation over RFCOMM
//! (Bluetooth Classic — an ordered byte stream, the same shape TCP
//! gives, unlike GATT/BLE's small-MTU characteristic model), wrapping
//! the `bluer` crate (BlueZ D-Bus bindings) — confined to this module
//! (and its own tests) per the wrap-third-party-deps rule
//! `flow_core::channel::Channel` exists to enforce.
//!
//! **Linux-only.** `bluer` wraps BlueZ, which only exists on Linux;
//! there is no equally mature high-level Bluetooth Classic RFCOMM crate
//! for macOS (would mean hand-written `IOBluetooth` bindings) or
//! Windows (the WinRT Bluetooth APIs) as of this writing — an honest gap
//! this crate doesn't attempt to paper over, same as `flow-platform`'s
//! E4-E7 platform caveats. Gated behind the `bluetooth` Cargo feature
//! (`daemon/Cargo.toml`), not built by default:
//!
//! ```sh
//! cargo build -p flow-daemon --features bluetooth
//! ```
//!
//! RFCOMM has no built-in message framing (unlike `TcpChannel`'s
//! WebSocket, which already frames text messages) — this module adds a
//! 4-byte big-endian length prefix ahead of each JSON-encoded
//! [`ChannelMessage`].

use std::str::FromStr;

use bluer::rfcomm::{SocketAddr as RfcommAddr, Stream};
use bluer::Address;
use flow_core::channel::{BluetoothAddr, Channel, ChannelError, ChannelKind, ChannelMessage};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The RFCOMM channel number Flow uses. An arbitrary, fixed choice
/// within RFCOMM's valid 1-30 range — real deployment would instead
/// negotiate this via an SDP service record (`bluer`'s
/// `Profile`/`bluetoothd` feature), deliberately deferred past this
/// task's scope; see this task's `buildNote`.
const RFCOMM_CHANNEL: u8 = 5;

/// A `Channel` backed by an RFCOMM (Bluetooth Classic) socket.
pub struct BluetoothChannel {
    stream: Stream,
}

impl BluetoothChannel {
    /// Connects to a peer daemon at `addr` on Flow's fixed RFCOMM
    /// channel.
    pub async fn connect(addr: BluetoothAddr) -> Result<Self, ChannelError> {
        let device_addr = parse_address(&addr)?;
        let stream = Stream::connect(RfcommAddr::new(device_addr, RFCOMM_CHANNEL))
            .await
            .map_err(|_| ChannelError::Unreachable)?;
        Ok(Self { stream })
    }

    /// Wraps an already-accepted RFCOMM stream (from a
    /// `bluer::rfcomm::Listener::accept()` loop bound to Flow's fixed
    /// channel — that loop lives with whichever code owns "listen for
    /// incoming peer connections", track G7, the same split
    /// `TcpChannel::accept` uses for its own `TcpListener`).
    pub fn accept(stream: Stream) -> Self {
        Self { stream }
    }
}

/// Reverses `BluetoothAddr`'s `Display`-formatted `bluer::Address`
/// string (`"AA:BB:CC:DD:EE:FF"`) back into the type `bluer`'s own API
/// needs. Pure and unit-testable without a real Bluetooth socket, unlike
/// everything else in this module.
fn parse_address(addr: &BluetoothAddr) -> Result<Address, ChannelError> {
    Address::from_str(&addr.0).map_err(|_| ChannelError::Unreachable)
}

#[async_trait::async_trait]
impl Channel for BluetoothChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Bluetooth
    }

    async fn send(&mut self, msg: ChannelMessage) -> Result<(), ChannelError> {
        let bytes =
            serde_json::to_vec(&msg).map_err(|err| ChannelError::Serialization(err.to_string()))?;
        let len = u32::try_from(bytes.len())
            .map_err(|_| ChannelError::Serialization("message exceeds 4GiB".to_string()))?;
        self.stream
            .write_u32(len)
            .await
            .map_err(|_| ChannelError::ConnectionLost)?;
        self.stream
            .write_all(&bytes)
            .await
            .map_err(|_| ChannelError::ConnectionLost)
    }

    async fn recv(&mut self) -> Result<ChannelMessage, ChannelError> {
        let len = self
            .stream
            .read_u32()
            .await
            .map_err(|_| ChannelError::ConnectionLost)?;
        let mut buf = vec![0u8; len as usize];
        self.stream
            .read_exact(&mut buf)
            .await
            .map_err(|_| ChannelError::ConnectionLost)?;
        serde_json::from_slice(&buf).map_err(|err| ChannelError::Serialization(err.to_string()))
    }

    async fn close(&mut self) -> Result<(), ChannelError> {
        self.stream
            .shutdown()
            .await
            .map_err(|_| ChannelError::ConnectionLost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_address_string_parses() {
        let addr = BluetoothAddr("AA:BB:CC:DD:EE:FF".to_string());
        let parsed = parse_address(&addr).expect("valid address");
        assert_eq!(parsed.to_string().to_uppercase(), "AA:BB:CC:DD:EE:FF");
    }

    #[test]
    fn an_invalid_address_string_is_rejected_not_panicking() {
        let addr = BluetoothAddr("not-a-bluetooth-address".to_string());
        assert_eq!(parse_address(&addr), Err(ChannelError::Unreachable));
    }

    /// Two local RFCOMM endpoints exchanging a hand-crafted message, per
    /// this task's acceptance criteria. Ignored by default: this
    /// session's own container has no Bluetooth support at the kernel
    /// level at all (confirmed directly — creating a raw `AF_BLUETOOTH`
    /// `SOCK_STREAM`/`BTPROTO_RFCOMM` socket here fails with `EAFNOSUPPORT`,
    /// "Address family not supported by protocol", independent of
    /// `bluetoothd` or a real adapter being present), so this can't run
    /// unattended here. Run explicitly on a Linux machine with a real
    /// Bluetooth adapter and `bluetoothd` running:
    ///
    /// ```sh
    /// cargo test -p flow-daemon --features bluetooth --lib channel::bluetooth -- --ignored
    /// ```
    #[ignore = "needs a real Bluetooth adapter + bluetoothd; this container's kernel has no AF_BLUETOOTH support at all"]
    #[tokio::test]
    async fn a_hand_crafted_heartbeat_round_trips_over_a_local_loopback_rfcomm_pair() {
        use bluer::rfcomm::Listener;
        use flow_core::protocol::{InputEvent, KeyboardEvent};

        let listener = Listener::bind(RfcommAddr::new(Address::any(), RFCOMM_CHANNEL))
            .await
            .expect("bind rfcomm listener");
        let local_addr = listener.as_ref().local_addr().expect("local addr");

        let server = tokio::spawn(async move {
            let (stream, _peer) = listener.accept().await.expect("accept");
            BluetoothChannel::accept(stream)
        });
        let mut client = BluetoothChannel::connect(BluetoothAddr(local_addr.addr.to_string()))
            .await
            .expect("connect");
        let mut server = server.await.expect("server task");

        let sent = ChannelMessage::Input {
            sequence: 1,
            event: InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "A".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            }),
        };
        client.send(sent.clone()).await.expect("send");
        let received = server.recv().await.expect("recv");
        assert_eq!(received, sent);
    }
}
