//! `up` mode end to end, the way the UI E2E suite drives it: start paired
//! devices, connect to each control server with its token (sent as the
//! WebSocket subprotocol), `switch_to` the controlled device, inject input
//! on the controller with `debug_inject_input`, and read it back from the
//! controlled device's virtual OS with `debug_virtual_io`.

use std::time::{Duration, Instant};

use flow_daemon::devmode::SecurityMode;
use flow_harness::up::{self, ManifestEntry, UpOptions};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A control-API client: one request at a time, events kept as they pass.
struct Client {
    ws: Ws,
    next_id: u64,
    devices: Vec<Value>,
}

impl Client {
    async fn connect(entry: &ManifestEntry) -> Self {
        let mut request = entry.ws_url.as_str().into_client_request().unwrap();
        request
            .headers_mut()
            .insert("sec-websocket-protocol", entry.token.parse().unwrap());
        let (ws, _) = tokio_tungstenite::connect_async(request)
            .await
            .unwrap_or_else(|e| panic!("{}: connect: {e}", entry.name));
        Self {
            ws,
            next_id: 0,
            devices: Vec::new(),
        }
    }

    /// Sends a command and returns its reply, recording events on the way.
    async fn call(&mut self, command: &str, payload: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id.to_string();
        let request = json!({ "id": id, "command": command, "payload": payload });
        self.ws
            .send(Message::Text(request.to_string().into()))
            .await
            .unwrap();
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(10), self.ws.next())
                .await
                .unwrap_or_else(|_| panic!("no reply to {command}"))
                .expect("connection open")
                .expect("frame");
            let Message::Text(text) = frame else { continue };
            let value: Value = serde_json::from_str(&text).unwrap();
            if value["event"] == "devices_changed" {
                self.devices = value["payload"].as_array().cloned().unwrap_or_default();
            }
            if value["id"] == id.as_str() {
                assert_eq!(value["ok"], true, "{command} failed: {value}");
                return value;
            }
        }
    }

    /// The id this device knows `name` by, from its `devices_changed` events.
    async fn device_id(&mut self, name: &str) -> String {
        // A harmless command drains the snapshot sent on connect.
        self.call("debug_virtual_io", Value::Null).await;
        self.devices
            .iter()
            .find(|d| d["name"] == name)
            .and_then(|d| d["id"].as_str())
            .unwrap_or_else(|| panic!("{name} not among {:?}", self.devices))
            .to_string()
    }
}

fn key(down: bool, name: &str, timestamp_ms: u64) -> Value {
    let kind = if down { "KeyDown" } else { "KeyUp" };
    json!({ "Keyboard": { kind: { "key": name, "modifiers": [], "timestamp_ms": timestamp_ms } } })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn injected_input_on_the_controller_reaches_the_controlled_virtual_os() {
    let options = UpOptions {
        devices: up::parse_devices("Mac:mac:controller,PC:win:controlled").unwrap(),
        paired: true,
        base_port: None,
        manifest: None,
        security: SecurityMode::Secure,
    };
    let (world, manifest) = up::start(&options).await.expect("devices come up");
    assert_eq!(manifest.len(), 2);
    let entry = |name: &str| manifest.iter().find(|e| e.name == name).unwrap();
    assert!(entry("Mac").ws_url.starts_with("ws://127.0.0.1:"));
    assert!(!entry("PC").token.is_empty());

    let mut mac = Client::connect(entry("Mac")).await;
    let mut pc = Client::connect(entry("PC")).await;
    pc.call("debug_virtual_io", json!({ "clear": true })).await;

    let target = mac.device_id("PC").await;
    mac.call("switch_to", json!({ "target": target })).await;
    for (i, (down, name)) in [(true, "A"), (false, "A"), (true, "B"), (false, "B")]
        .into_iter()
        .enumerate()
    {
        mac.call("debug_inject_input", key(down, name, i as u64 + 1))
            .await;
    }

    let deadline = Instant::now() + Duration::from_secs(10);
    let injected = loop {
        let reply = pc.call("debug_virtual_io", Value::Null).await;
        let keys: Vec<String> = reply["result"]["injected"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| {
                let (kind, body) = e["Keyboard"].as_object()?.iter().next()?;
                Some(format!("{kind} {}", body["key"].as_str()?))
            })
            .collect();
        if keys.len() >= 4 || Instant::now() >= deadline {
            break keys;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(injected, ["KeyDown A", "KeyUp A", "KeyDown B", "KeyUp B"]);

    // Nothing reached the Mac's own applications while it was remote.
    let mac_io = mac.call("debug_virtual_io", Value::Null).await;
    assert_eq!(mac_io["result"]["delivered_locally"], json!([]));
    assert_eq!(mac_io["result"]["suppressed"], true);
    world.shutdown();
}
