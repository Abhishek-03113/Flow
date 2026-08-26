//! End-to-end proof that `NoiseChannel` (`daemon/todos.json` H3) never
//! puts a `ChannelMessage`'s plaintext on the wire — this task's own
//! acceptance criterion, verified by a byte-level packet sniff (a real
//! TCP relay that records every byte it forwards), not by asserting on
//! the decoded message.

use std::sync::Arc;

use flow_core::channel::{Channel, ChannelMessage};
use flow_core::protocol::{InputEvent, KeyboardEvent};
use flow_daemon::channel::noise::NoiseChannel;
use flow_daemon::channel::tcp::TcpChannel;
use flow_daemon::identity::DeviceIdentity;
use flow_daemon::storage::Storage;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// A marker distinctive enough that it can't plausibly appear in Noise
/// handshake material, WebSocket framing, or JSON structure by chance.
const MARKER: &str = "TOTALLY-SECRET-PLAINTEXT-MARKER-8f3c1e";

async fn an_identity() -> DeviceIdentity {
    let storage = Storage::open_in_memory().await.expect("open db");
    DeviceIdentity::load_or_generate(storage).await
}

/// Binds a TCP proxy in front of `target`: every byte a client sends to
/// the proxy is forwarded to `target` (and vice versa), while also being
/// appended to the returned buffer — a real, if minimal, packet sniff
/// sitting on the actual wire between the two `TcpChannel` endpoints.
async fn spawn_sniffing_proxy(
    target: std::net::SocketAddr,
) -> (std::net::SocketAddr, Arc<Mutex<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind proxy listener");
    let proxy_addr = listener.local_addr().expect("proxy local addr");
    let captured = Arc::new(Mutex::new(Vec::new()));

    let captured_for_task = captured.clone();
    tokio::spawn(async move {
        let (client_stream, _peer) = listener.accept().await.expect("accept from client");
        let server_stream = TcpStream::connect(target)
            .await
            .expect("connect to real server");

        let (mut client_read, mut client_write) = client_stream.into_split();
        let (mut server_read, mut server_write) = server_stream.into_split();

        let captured_c2s = captured_for_task.clone();
        let client_to_server = async move {
            let mut buf = [0u8; 4096];
            loop {
                let n = client_read.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                captured_c2s.lock().await.extend_from_slice(&buf[..n]);
                if server_write.write_all(&buf[..n]).await.is_err() {
                    break;
                }
            }
        };
        let captured_s2c = captured_for_task.clone();
        let server_to_client = async move {
            let mut buf = [0u8; 4096];
            loop {
                let n = server_read.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                captured_s2c.lock().await.extend_from_slice(&buf[..n]);
                if client_write.write_all(&buf[..n]).await.is_err() {
                    break;
                }
            }
        };
        tokio::join!(client_to_server, server_to_client);
    });

    (proxy_addr, captured)
}

fn contains_marker(haystack: &[u8]) -> bool {
    haystack
        .windows(MARKER.len())
        .any(|window| window == MARKER.as_bytes())
}

fn a_marker_event() -> ChannelMessage {
    ChannelMessage::Input {
        sequence: 1,
        event: InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: MARKER.to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        }),
    }
}

#[tokio::test]
async fn a_byte_level_sniff_never_reveals_plaintext_through_a_noise_channel() {
    let server_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind real server");
    let server_addr = server_listener.local_addr().expect("server addr");
    let (proxy_addr, captured) = spawn_sniffing_proxy(server_addr).await;

    let identity_a = an_identity().await;
    let identity_b = an_identity().await;

    let server = tokio::spawn(async move {
        let (stream, _peer) = server_listener.accept().await.expect("accept");
        let tcp = TcpChannel::accept(stream).await.expect("accept ws");
        let mut noise = NoiseChannel::accept(tcp, &identity_b)
            .await
            .expect("noise accept");
        noise.recv().await.expect("recv")
    });

    let tcp_client = TcpChannel::connect(proxy_addr)
        .await
        .expect("connect via proxy");
    let mut client = NoiseChannel::initiate(tcp_client, &identity_a)
        .await
        .expect("noise initiate");

    let sent = a_marker_event();
    client.send(sent.clone()).await.expect("send");

    let received = server.await.expect("server task");
    assert_eq!(received, sent);

    let bytes = captured.lock().await.clone();
    assert!(
        !contains_marker(&bytes),
        "the marker plaintext leaked onto the wire ({} bytes captured)",
        bytes.len()
    );
}

/// A negative control for the test above: sends the identical marker
/// event over a *plain* `TcpChannel` (no `NoiseChannel` wrapper) through
/// the same sniffing proxy, and confirms the marker *does* show up.
/// Without this, a bug in the sniff itself (e.g. the proxy silently not
/// forwarding data) could make the encrypted test pass for the wrong
/// reason — this proves the methodology would actually catch a real
/// plaintext leak.
///
/// Sent server -> client, not client -> server: RFC 6455 requires every
/// client-to-server WebSocket frame to be XOR-masked with a random
/// per-frame key (server-to-client frames are not), so a literal
/// byte-search over a masked frame wouldn't find the marker even
/// completely unencrypted — that's WebSocket's own obfuscation, unrelated
/// to `NoiseChannel`, and would make this control meaningless if sent in
/// the masked direction.
#[tokio::test]
async fn the_sniff_methodology_does_detect_plaintext_on_an_unencrypted_channel() {
    let server_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind real server");
    let server_addr = server_listener.local_addr().expect("server addr");
    let (proxy_addr, captured) = spawn_sniffing_proxy(server_addr).await;

    let sent = a_marker_event();
    let to_send = sent.clone();
    let server = tokio::spawn(async move {
        let (stream, _peer) = server_listener.accept().await.expect("accept");
        let mut tcp = TcpChannel::accept(stream).await.expect("accept ws");
        tcp.send(to_send).await.expect("send");
    });

    let mut client = TcpChannel::connect(proxy_addr)
        .await
        .expect("connect via proxy");
    let received = client.recv().await.expect("recv");
    assert_eq!(received, sent);
    server.await.expect("server task");

    let bytes = captured.lock().await.clone();
    assert!(
        contains_marker(&bytes),
        "the sniff should have observed the marker in plaintext WebSocket/JSON traffic"
    );
}
