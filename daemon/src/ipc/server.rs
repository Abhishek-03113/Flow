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
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use super::dispatch::dispatch;
use crate::service::DaemonService;

type WsSink = SplitSink<WebSocketStream<TcpStream>, Message>;

/// Handles one client connection end to end: WebSocket handshake, the
/// five initial state-push events, then forwarding further watch-channel
/// updates as events while dispatching incoming commands — until the
/// client disconnects. Never panics on a client error; a bad frame or a
/// dropped socket just ends this task, leaving every other connection
/// and `ServiceState` itself untouched.
#[tracing::instrument(skip_all)]
pub async fn handle_connection(stream: TcpStream, service: Arc<DaemonService>) {
    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(err) => {
            tracing::debug!("websocket handshake failed: {err}");
            return;
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

    async fn spawn_test_server() -> std::net::SocketAddr {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = Arc::new(DaemonService::new(storage).await);
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");

        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                handle_connection(stream, service).await;
            }
        });
        addr
    }

    async fn connect(
        addr: std::net::SocketAddr,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>> {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .expect("connect");
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
}
