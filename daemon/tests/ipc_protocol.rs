//! End-to-end proof of the IPC contract (`daemon/todos.json` task C6):
//! the real `flow-daemon` service and WebSocket listener, spawned
//! in-process (not a subprocess), driven by a raw `tokio-tungstenite`
//! client asserting the exact JSON shape of a full session — connect,
//! the 5 initial events, one of each of the 9 commands, and at least 2
//! error-code paths. The Rust-side equivalent of what track D proves
//! from the Dart side against a real daemon.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use flow_daemon::ipc::server::handle_connection;
use flow_daemon::service::DaemonService;
use flow_daemon::storage::Storage;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn spawn_daemon() -> SocketAddr {
    let storage = Storage::open_in_memory().await.expect("open in-memory db");
    let service = Arc::new(DaemonService::new(storage).await);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let service = Arc::clone(&service);
            tokio::spawn(async move {
                handle_connection(stream, service).await;
            });
        }
    });
    addr
}

async fn connect(addr: SocketAddr) -> Client {
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
        .await
        .expect("connect");
    ws
}

async fn recv_json(ws: &mut Client) -> Value {
    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("timed out waiting for a frame")
        .expect("stream ended unexpectedly")
        .expect("frame error");
    serde_json::from_str(&msg.into_text().expect("text frame")).expect("valid json")
}

async fn send_command(ws: &mut Client, id: &str, command: &str, payload: Value) {
    let request = json!({ "id": id, "command": command, "payload": payload });
    ws.send(Message::Text(request.to_string().into()))
        .await
        .expect("send request");
}

/// Reads exactly `count` frames and splits out the ack/err matching
/// `id` from any event frames among them. `count` must match the
/// caller's own knowledge of how many frames a given command produces
/// (e.g. an ack plus the one state-change event it triggers) — the ack
/// and its event can arrive in either order (`daemon-ipc.md`'s command
/// table doesn't promise one before the other), so this doesn't stop as
/// soon as the ack is seen; it drains the whole expected batch.
async fn recv_frames(ws: &mut Client, id: &str, count: usize) -> (Value, Vec<Value>) {
    let mut events = Vec::new();
    let mut ack = None;
    for _ in 0..count {
        let value = recv_json(ws).await;
        if value.get("id").and_then(Value::as_str) == Some(id) {
            ack = Some(value);
        } else {
            events.push(value);
        }
    }
    (ack.expect("ack/err frame for this request id"), events)
}

#[tokio::test]
async fn full_session_matches_the_contract_envelope_byte_for_byte() {
    let addr = spawn_daemon().await;
    let mut ws = connect(addr).await;

    // 1. Connect -> exactly 5 initial events, in the fixed order, before
    // anything else — "connecting... is the initial fetch".
    let expected_initial_events = [
        "devices_changed",
        "link_state_changed",
        "pairing_session_changed",
        "settings_changed",
        "permission_changed",
    ];
    for expected in expected_initial_events {
        let value = recv_json(&mut ws).await;
        assert_eq!(value["event"], expected);
    }

    // 2. switch_active_device -> ack + devices_changed reflecting it.
    send_command(
        &mut ws,
        "req-1",
        "switch_active_device",
        json!({ "device_id": "d2" }),
    )
    .await;
    let (ack, events) = recv_frames(&mut ws, "req-1", 2).await;
    assert_eq!(ack, json!({ "id": "req-1", "ok": true }));
    let devices = events
        .iter()
        .find(|e| e["event"] == "devices_changed")
        .expect("devices_changed event")["payload"]
        .clone();
    let d2 = devices
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == "d2")
        .unwrap();
    assert_eq!(d2["state"], "active");

    // 3. remove_device -> ack + devices_changed with the device gone.
    send_command(
        &mut ws,
        "req-2",
        "remove_device",
        json!({ "device_id": "d3" }),
    )
    .await;
    let (ack, events) = recv_frames(&mut ws, "req-2", 2).await;
    assert_eq!(ack, json!({ "id": "req-2", "ok": true }));
    let devices = events
        .iter()
        .find(|e| e["event"] == "devices_changed")
        .expect("devices_changed event")["payload"]
        .clone();
    assert!(!devices.as_array().unwrap().iter().any(|d| d["id"] == "d3"));

    // 4. start_pairing -> ack (session moves out of idle immediately).
    send_command(&mut ws, "req-3", "start_pairing", Value::Null).await;
    let (ack, _events) = recv_frames(&mut ws, "req-3", 2).await;
    assert_eq!(ack, json!({ "id": "req-3", "ok": true }));

    // 5. cancel_pairing -> ack, back to idle before the search timer fires.
    send_command(&mut ws, "req-4", "cancel_pairing", Value::Null).await;
    let (ack, _events) = recv_frames(&mut ws, "req-4", 2).await;
    assert_eq!(ack, json!({ "id": "req-4", "ok": true }));

    // 6. pair_with_candidate while idle (not found) -> error path #1.
    send_command(
        &mut ws,
        "req-5",
        "pair_with_candidate",
        json!({ "candidate_id": "cand-office-mini" }),
    )
    .await;
    let (err, _events) = recv_frames(&mut ws, "req-5", 1).await;
    assert_eq!(
        err,
        json!({
            "id": "req-5",
            "ok": false,
            "error": { "code": "pairing_not_ready", "message": err["error"]["message"] }
        })
    );

    // 7. set_switch_key -> ack + settings_changed.
    send_command(
        &mut ws,
        "req-6",
        "set_switch_key",
        json!({ "label": "F13", "keys": ["F13"] }),
    )
    .await;
    let (ack, events) = recv_frames(&mut ws, "req-6", 2).await;
    assert_eq!(ack, json!({ "id": "req-6", "ok": true }));
    let settings = events
        .iter()
        .find(|e| e["event"] == "settings_changed")
        .expect("settings_changed event")["payload"]
        .clone();
    assert_eq!(settings["switch_key"]["label"], "F13");

    // 8. update_settings -> ack + settings_changed with only that field.
    send_command(
        &mut ws,
        "req-7",
        "update_settings",
        json!({ "share_mouse": false }),
    )
    .await;
    let (ack, events) = recv_frames(&mut ws, "req-7", 2).await;
    assert_eq!(ack, json!({ "id": "req-7", "ok": true }));
    let settings = events
        .iter()
        .find(|e| e["event"] == "settings_changed")
        .expect("settings_changed event")["payload"]
        .clone();
    assert_eq!(settings["share_mouse"], false);

    // 9. reset_settings -> ack + settings_changed restoring defaults.
    send_command(&mut ws, "req-8", "reset_settings", Value::Null).await;
    let (ack, events) = recv_frames(&mut ws, "req-8", 2).await;
    assert_eq!(ack, json!({ "id": "req-8", "ok": true }));
    let settings = events
        .iter()
        .find(|e| e["event"] == "settings_changed")
        .expect("settings_changed event")["payload"]
        .clone();
    assert_eq!(settings["switch_key"]["label"], "Scroll Lock");
    assert_eq!(settings["share_mouse"], true);

    // 10. request_permission -> ack + permission_changed (granted).
    send_command(&mut ws, "req-9", "request_permission", Value::Null).await;
    let (ack, events) = recv_frames(&mut ws, "req-9", 2).await;
    assert_eq!(ack, json!({ "id": "req-9", "ok": true }));
    let permission = events
        .iter()
        .find(|e| e["event"] == "permission_changed")
        .expect("permission_changed event")["payload"]
        .clone();
    assert_eq!(permission["granted"], true);

    // 11. request_permission again -> error path #2.
    send_command(&mut ws, "req-10", "request_permission", Value::Null).await;
    let (err, _events) = recv_frames(&mut ws, "req-10", 1).await;
    assert_eq!(
        err,
        json!({
            "id": "req-10",
            "ok": false,
            "error": { "code": "permission_already_granted", "message": err["error"]["message"] }
        })
    );
}
