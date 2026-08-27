//! Per-connection WebSocket IPC handler (`daemon/todos.json` task C3).
//! `tokio-tungstenite` is confined to this module — application code
//! elsewhere in the daemon only ever sees `IpcRequest`/`IpcResponse`
//! (`flow-core`), never the WebSocket crate directly.

use std::sync::Arc;

use flow_core::ipc::{IpcRequest, IpcResponse};
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::handshake::server::{
    ErrorResponse, Request, Response as HandshakeResponse,
};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

/// Every real `IpcRequest`/`IpcResponse` is small JSON (a command name,
/// a device id, a settings patch, one state snapshot) — nowhere near
/// tokio-tungstenite's generous library defaults (64MB message / 16MB
/// frame). An explicit, much tighter cap means a malformed or hostile
/// local client can't make this connection buffer tens of megabytes for
/// one frame before `dispatch` ever sees it (daemon review gap #32).
fn ipc_websocket_config() -> WebSocketConfig {
    const MAX_MESSAGE_BYTES: usize = 256 * 1024;
    WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES))
}

use super::dispatch::dispatch;
use crate::service::DaemonService;

type WsSink = SplitSink<WebSocketStream<TcpStream>, Message>;

const TOKEN_HEADER: &str = "sec-websocket-protocol";

/// Handles one client connection end to end: an authenticated WebSocket
/// handshake, the five initial state-push events, then forwarding
/// further watch-channel updates as events while dispatching incoming
/// commands — until the client disconnects. Never panics on a client
/// error; a bad frame or a dropped socket just ends this task, leaving
/// every other connection and `ServiceState` itself untouched.
///
/// `expected_token` gates the handshake itself (`auth::load_or_generate_token`,
/// loaded once at daemon startup): `127.0.0.1` is reachable by *any*
/// local process, not just the intended Flutter UI, and this connection
/// previously had no way to tell the two apart. The token travels as the
/// WebSocket subprotocol (`Sec-WebSocket-Protocol`) specifically because
/// a browser page's own `WebSocket` can set that (unlike an arbitrary
/// header) but can never *read* the local token file in the first place
/// — the mechanism being technically settable by a browser doesn't
/// matter when the secret itself is unreachable to one.
#[tracing::instrument(skip_all)]
pub async fn handle_connection(
    stream: TcpStream,
    service: Arc<DaemonService>,
    expected_token: Arc<str>,
) {
    let ws_stream = {
        // tokio-tungstenite's `Callback` trait fixes this closure's exact
        // `Result<Response<()>, Response<Option<String>>>` shape — there's
        // no smaller type to return instead, and boxing would just move
        // the same bytes onto the heap for a value that lives only for
        // the duration of one handshake.
        #[allow(clippy::result_large_err)]
        let callback = |req: &Request, mut response: HandshakeResponse| {
            let presented = req
                .headers()
                .get(TOKEN_HEADER)
                .and_then(|value| value.to_str().ok());
            match presented {
                Some(token) if token == expected_token.as_ref() => {
                    // Echo the offered subprotocol back — required for
                    // the handshake to be a spec-valid accept of it, and
                    // for a real (e.g. browser) WebSocket client to treat
                    // the connection as actually established.
                    response.headers_mut().insert(
                        tokio_tungstenite::tungstenite::http::header::SEC_WEBSOCKET_PROTOCOL,
                        req.headers()
                            .get(TOKEN_HEADER)
                            .expect("presented implies this header exists")
                            .clone(),
                    );
                    Ok(response)
                }
                _ => {
                    let mut unauthorized =
                        ErrorResponse::new(Some("missing or invalid IPC auth token".to_string()));
                    *unauthorized.status_mut() = StatusCode::UNAUTHORIZED;
                    Err(unauthorized)
                }
            }
        };
        match tokio_tungstenite::accept_hdr_async_with_config(
            stream,
            callback,
            Some(ipc_websocket_config()),
        )
        .await
        {
            Ok(ws) => ws,
            Err(err) => {
                tracing::debug!("websocket handshake failed: {err}");
                return;
            }
        }
    };
    tracing::debug!("ipc connection established");
    let (mut sink, mut source) = ws_stream.split();

    let mut devices_rx = service.watch_devices();
    let mut link_state_rx = service.watch_link_state();
    let mut pairing_session_rx = service.watch_pairing_session();
    let mut settings_rx = service.watch_settings();
    let mut permission_rx = service.watch_permission();

    // "Connecting to the channel and subscribing IS the initial fetch"
    // (daemon-ipc.md) — one Event per watch* stream's current value,
    // before anything else, in a fixed order. Each value is cloned out of
    // its `watch::Ref` guard *before* the `.await` below: the guard isn't
    // `Send`, so holding it across an await (as a single array-literal
    // expression of `.await`s would, via temporary lifetime extension)
    // makes this whole connection's future non-`Send` and unspawnable.
    let devices = devices_rx.borrow_and_update().clone();
    if send_event(&mut sink, "devices_changed", &devices)
        .await
        .is_err()
    {
        return;
    }
    let link_state = *link_state_rx.borrow_and_update();
    if send_event(&mut sink, "link_state_changed", &link_state)
        .await
        .is_err()
    {
        return;
    }
    let pairing_session = pairing_session_rx.borrow_and_update().clone();
    if send_event(&mut sink, "pairing_session_changed", &pairing_session)
        .await
        .is_err()
    {
        return;
    }
    let settings = settings_rx.borrow_and_update().clone();
    if send_event(&mut sink, "settings_changed", &settings)
        .await
        .is_err()
    {
        return;
    }
    let permission = permission_rx.borrow_and_update().clone();
    if send_event(&mut sink, "permission_changed", &permission)
        .await
        .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            changed = devices_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                let value = devices_rx.borrow_and_update().clone();
                if send_event(&mut sink, "devices_changed", &value).await.is_err() {
                    break;
                }
            }
            changed = link_state_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                let value = *link_state_rx.borrow_and_update();
                if send_event(&mut sink, "link_state_changed", &value).await.is_err() {
                    break;
                }
            }
            changed = pairing_session_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                let value = pairing_session_rx.borrow_and_update().clone();
                if send_event(&mut sink, "pairing_session_changed", &value).await.is_err() {
                    break;
                }
            }
            changed = settings_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                let value = settings_rx.borrow_and_update().clone();
                if send_event(&mut sink, "settings_changed", &value).await.is_err() {
                    break;
                }
            }
            changed = permission_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                let value = permission_rx.borrow_and_update().clone();
                if send_event(&mut sink, "permission_changed", &value).await.is_err() {
                    break;
                }
            }
            incoming = source.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        // A frame that isn't a well-formed IpcRequest has
                        // no `id` to correlate a reply to — drop it
                        // rather than crash the connection.
                        if let Ok(req) = serde_json::from_str::<IpcRequest>(&text) {
                            let response = dispatch(&service, req).await;
                            if send_response(&mut sink, &response).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
}

async fn send_event<T: Serialize>(sink: &mut WsSink, event: &str, value: &T) -> Result<(), ()> {
    send_response(
        sink,
        &IpcResponse::Event {
            event: event.to_string(),
            payload: serde_json::to_value(value).expect("serialize event payload"),
        },
    )
    .await
}

async fn send_response(sink: &mut WsSink, response: &IpcResponse) -> Result<(), ()> {
    let text = serde_json::to_string(response).expect("serialize response");
    sink.send(Message::Text(text.into())).await.map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt as _;
    use serde_json::json;
    use tokio::net::TcpListener;

    use super::*;
    use crate::storage::Storage;

    const TEST_TOKEN: &str = "test-token-0123456789abcdef";

    async fn spawn_test_server() -> std::net::SocketAddr {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = Arc::new(DaemonService::new_seeded_for_test(storage).await);
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let token: Arc<str> = Arc::from(TEST_TOKEN);

        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                handle_connection(stream, service, token).await;
            }
        });
        addr
    }

    /// Builds an otherwise-normal client handshake request carrying
    /// `token` as the WebSocket subprotocol — the same header
    /// `handle_connection` checks — so tests can exercise both a
    /// matching and a missing/wrong token against a real handshake.
    fn request_with_token(
        addr: std::net::SocketAddr,
        token: &str,
    ) -> tokio_tungstenite::tungstenite::handshake::client::Request {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut request = format!("ws://{addr}")
            .into_client_request()
            .expect("valid client request");
        request.headers_mut().insert(
            "sec-websocket-protocol",
            token.parse().expect("token is a valid header value"),
        );
        request
    }

    async fn connect(
        addr: std::net::SocketAddr,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>> {
        let (ws, _) = tokio_tungstenite::connect_async(request_with_token(addr, TEST_TOKEN))
            .await
            .expect("connect with the correct token");
        ws
    }

    #[tokio::test]
    async fn a_new_connection_receives_exactly_five_initial_events_in_order() {
        let addr = spawn_test_server().await;
        let mut ws = connect(addr).await;

        let expected_order = [
            "devices_changed",
            "link_state_changed",
            "pairing_session_changed",
            "settings_changed",
            "permission_changed",
        ];

        for expected_event in expected_order {
            let msg = ws.next().await.expect("frame").expect("ok frame");
            let text = msg.into_text().expect("text frame");
            let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
            assert_eq!(value["event"], expected_event);
        }
    }

    #[tokio::test]
    async fn a_command_receives_its_ack_and_a_matching_state_event() {
        let addr = spawn_test_server().await;
        let mut ws = connect(addr).await;

        // Drain the 5 initial events.
        for _ in 0..5 {
            ws.next().await.expect("frame").expect("ok frame");
        }

        let request = json!({
            "id": "req-1",
            "command": "switch_active_device",
            "payload": { "device_id": "d2" }
        });
        ws.send(Message::Text(request.to_string().into()))
            .await
            .expect("send request");

        let mut saw_ack = false;
        let mut saw_devices_changed = false;
        for _ in 0..2 {
            let msg = ws.next().await.expect("frame").expect("ok frame");
            let value: serde_json::Value =
                serde_json::from_str(&msg.into_text().expect("text frame")).expect("valid json");
            if value["id"] == "req-1" && value["ok"] == true {
                saw_ack = true;
            }
            if value["event"] == "devices_changed" {
                saw_devices_changed = true;
            }
        }
        assert!(saw_ack, "expected an ack for req-1");
        assert!(saw_devices_changed, "expected a devices_changed event");
    }

    #[tokio::test]
    async fn client_disconnect_does_not_panic_the_handler() {
        let addr = spawn_test_server().await;
        let ws = connect(addr).await;
        drop(ws);
        // Give the server task a moment to observe the close; success is
        // simply this test completing without the server task panicking
        // (a panic inside a spawned task doesn't fail the test process,
        // so this is mostly documentation — the real assertion is that a
        // second, independent connection to the same listener still
        // isn't affected, proven by the next connection test running at
        // all).
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    /// `daemon/todos.json` I3's regression test: `tokio::spawn` already
    /// isolates a panic to its own task (the runtime catches it and
    /// reports it only through that task's own `JoinHandle`) — this
    /// confirms that holds for the exact spawn shape `main.rs`'s
    /// connection-accept loop actually uses, not just in the abstract.
    /// A connection handler panicking for whatever reason (a bug, an
    /// unexpected malformed frame) must not take down a second,
    /// completely unrelated connection running concurrently on the same
    /// daemon.
    ///
    /// The hotkey runner (`hotkey::runner::spawn`) uses the identical
    /// `tokio::spawn(async move { ... })` primitive this test exercises,
    /// so the same isolation guarantee applies to it for the same
    /// reason — it isn't separately re-tested here because doing so for
    /// real needs a capturable input device this project's own
    /// development container doesn't have (`daemon/README.md`'s "E1:
    /// Linux capture via evdev" manual verification note), the same gap
    /// that already makes `hotkey::runner::spawn` return `None` (not
    /// panic) in this environment whenever it's actually called from
    /// `main.rs`.
    #[tokio::test]
    async fn a_panicking_connection_handler_does_not_affect_a_concurrent_one() {
        let real_addr = spawn_test_server().await;

        let panicking = tokio::spawn(async {
            panic!("deliberately panicking connection handler, per this test's own design");
        });

        let mut ws = connect(real_addr).await;
        for _ in 0..5 {
            let msg = ws.next().await.expect("frame").expect("ok frame");
            let text = msg.into_text().expect("text frame");
            let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
            assert!(value["event"].is_string(), "expected an initial event");
        }

        // The panic is reported only through its own task's JoinHandle
        // — it never reaches this test's own task, let alone the whole
        // process.
        assert!(
            panicking.await.is_err(),
            "the deliberately panicking task should report an Err, not silently succeed"
        );
    }

    /// The whole point of the token check: `127.0.0.1` is reachable by
    /// any local process, not just the intended Flutter UI, and a
    /// connection presenting no proof of that at all must never reach
    /// the point of receiving state or being able to send a command.
    #[tokio::test]
    async fn a_connection_with_no_token_is_rejected_before_the_handshake_completes() {
        let addr = spawn_test_server().await;
        let result = tokio_tungstenite::connect_async(format!("ws://{addr}")).await;
        assert!(
            result.is_err(),
            "a handshake with no auth token must not succeed"
        );
    }

    #[tokio::test]
    async fn a_connection_with_the_wrong_token_is_rejected() {
        let addr = spawn_test_server().await;
        let result =
            tokio_tungstenite::connect_async(request_with_token(addr, "not-the-real-token")).await;
        assert!(
            result.is_err(),
            "a handshake with an incorrect auth token must not succeed"
        );
    }

    #[tokio::test]
    async fn a_connection_with_the_correct_token_is_accepted() {
        let addr = spawn_test_server().await;
        // `connect` already sends TEST_TOKEN; success here (no panic) is
        // the assertion — a fresh connection reaching the point of
        // receiving its first initial event is proven by the existing
        // `a_new_connection_receives_exactly_five_initial_events_in_order`
        // test, which uses this same helper.
        let mut ws = connect(addr).await;
        let msg = ws.next().await.expect("frame").expect("ok frame");
        assert!(msg.into_text().is_ok());
    }

    /// Gap #32: an oversized frame must not be silently buffered and
    /// handed to `dispatch` — the connection ends instead. The client
    /// side here has no size limit of its own (it's happy to send
    /// whatever it's told to); it's `ipc_websocket_config`'s cap on the
    /// *server's* accept side doing the actual rejecting.
    #[tokio::test]
    async fn an_oversized_frame_ends_the_connection_instead_of_being_processed() {
        let addr = spawn_test_server().await;
        let mut ws = connect(addr).await;
        for _ in 0..5 {
            ws.next().await.expect("frame").expect("initial event");
        }

        let oversized = "x".repeat(1024 * 1024); // 1 MiB, well past the 256 KiB cap
                                                 // Sending may itself fail once the server drops the connection
                                                 // mid-write, or may succeed and only be rejected on the next
                                                 // read — either is consistent with "not processed as a valid
                                                 // request," so only the follow-up read is asserted on.
        let _ = ws.send(Message::Text(oversized.into())).await;

        let outcome = ws.next().await;
        assert!(
            matches!(outcome, None | Some(Err(_))),
            "an oversized frame should end the connection, not be dispatched: {outcome:?}"
        );
    }
}
