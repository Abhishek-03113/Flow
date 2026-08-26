//! [`TcpChannel`]: the `Channel` implementation over Wi-Fi/local network
//! (`docs/architecture/channels.md` "TcpChannel"), wrapping
//! `tokio-tungstenite` — confined to this module (and its own tests) per
//! the wrap-third-party-deps rule `flow_core::channel::Channel` exists
//! to enforce. Distinct from track C's local IPC WebSocket server
//! (`crate::ipc::server`): different port, a peer daemon rather than the
//! Flutter UI, and `ChannelMessage` traffic rather than
//! `IpcRequest`/`IpcResponse`.

use std::net::SocketAddr;

use flow_core::channel::{Channel, ChannelError, ChannelKind, ChannelMessage};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// A `Channel` backed by a WebSocket over a plain TCP connection.
pub struct TcpChannel {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl TcpChannel {
    /// Connects to a peer daemon already listening at `addr`.
    pub async fn connect(addr: SocketAddr) -> Result<Self, ChannelError> {
        let (stream, _response) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .map_err(|_| ChannelError::Unreachable)?;
        Ok(Self { stream })
    }

    /// Completes the server side of the WebSocket handshake on an
    /// already-accepted TCP connection (from a `TcpListener::accept()`
    /// in the daemon's peer-listening loop, track G3/G7).
    pub async fn accept(stream: TcpStream) -> Result<Self, ChannelError> {
        let ws_stream = tokio_tungstenite::accept_async(MaybeTlsStream::Plain(stream))
            .await
            .map_err(|_| ChannelError::ConnectionLost)?;
        Ok(Self { stream: ws_stream })
    }
}

#[async_trait::async_trait]
impl Channel for TcpChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Tcp
    }

    async fn send(&mut self, msg: ChannelMessage) -> Result<(), ChannelError> {
        let text = serde_json::to_string(&msg)
            .map_err(|err| ChannelError::Serialization(err.to_string()))?;
        self.stream
            .send(Message::Text(text.into()))
            .await
            .map_err(|_| ChannelError::ConnectionLost)
    }

    async fn recv(&mut self) -> Result<ChannelMessage, ChannelError> {
        loop {
            match self.stream.next().await {
                Some(Ok(Message::Text(text))) => {
                    return serde_json::from_str(&text)
                        .map_err(|err| ChannelError::Serialization(err.to_string()));
                }
                Some(Ok(Message::Close(_))) | None => return Err(ChannelError::ConnectionLost),
                // Ping/Pong/Binary frames aren't ChannelMessage traffic;
                // tungstenite answers pings automatically, so these are
                // just skipped rather than treated as an error.
                Some(Ok(_)) => continue,
                Some(Err(_)) => return Err(ChannelError::ConnectionLost),
            }
        }
    }

    async fn close(&mut self) -> Result<(), ChannelError> {
        self.stream
            .close(None)
            .await
            .map_err(|_| ChannelError::ConnectionLost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow_core::protocol::{InputEvent, KeyboardEvent};
    use tokio::net::TcpListener;

    async fn connected_pair() -> (TcpChannel, TcpChannel) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");

        let server = tokio::spawn(async move {
            let (stream, _peer) = listener.accept().await.expect("accept");
            TcpChannel::accept(stream).await.expect("server handshake")
        });
        let client = TcpChannel::connect(addr).await.expect("client connect");
        let server = server.await.expect("server task");
        (client, server)
    }

    #[tokio::test]
    async fn a_hand_crafted_keydown_is_received_verbatim() {
        let (mut client, mut server) = connected_pair().await;

        let sent = ChannelMessage::Input {
            sequence: 1,
            event: InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "A".to_string(),
                modifiers: vec![],
                timestamp_ms: 123,
            }),
        };
        client.send(sent.clone()).await.expect("send");

        let received = server.recv().await.expect("recv");
        assert_eq!(received, sent);
    }

    #[tokio::test]
    async fn a_heartbeat_round_trips_in_either_direction() {
        let (mut client, mut server) = connected_pair().await;

        server.send(ChannelMessage::Heartbeat).await.expect("send");
        assert_eq!(
            client.recv().await.expect("recv"),
            ChannelMessage::Heartbeat
        );
    }

    #[tokio::test]
    async fn both_ends_report_kind_tcp() {
        let (client, server) = connected_pair().await;
        assert_eq!(client.kind(), ChannelKind::Tcp);
        assert_eq!(server.kind(), ChannelKind::Tcp);
    }

    #[tokio::test]
    async fn recv_after_the_peer_closes_reports_connection_lost() {
        let (mut client, mut server) = connected_pair().await;
        client.close().await.expect("close");
        assert_eq!(server.recv().await, Err(ChannelError::ConnectionLost));
    }
}
