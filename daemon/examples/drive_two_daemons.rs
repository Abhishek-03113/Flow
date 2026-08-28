//! End-to-end driver for two locally-running `flow-daemon` instances
//! (`docs/testing/ui-test-branch-plan.md`). Connects to both over their
//! local IPC WebSocket, pairs A↔B, switches the active device, injects a
//! hardcoded key sequence and mouse path each direction via the
//! `debug_inject_input` test hook, exercises a graceful control switch,
//! and prints a PASS/FAIL matrix.
//!
//! Receiver-side confirmation is by grepping each daemon's `flow::hop`
//! log for the injection record, so run both daemons with `FLOW_TRACE=1`
//! and their stdout+stderr redirected to files, and point this driver at
//! those files:
//!
//! ```sh
//! # daemon A (defaults: IPC 47823, token ~/.flow/ipc.token)
//! FLOW_TRACE=1 FLOW_TEST_HOOKS=1 FLOW_SECURITY=insecure FLOW_DEV=1 \
//!   cargo run -p flow-daemon >a.log 2>&1 &
//!
//! # daemon B
//! FLOW_DEVICE_NAME=Flow-B FLOW_DATA_DIR=/tmp/flow-b FLOW_IPC_PORT=47833 \
//!   FLOW_IPC_TOKEN_PATH=/tmp/flow-b/ipc.token \
//!   FLOW_TRACE=1 FLOW_TEST_HOOKS=1 FLOW_SECURITY=insecure FLOW_DEV=1 \
//!   cargo run -p flow-daemon >b.log 2>&1 &
//!
//! FLOW_A_LOG=a.log \
//! FLOW_B_IPC_PORT=47833 FLOW_B_TOKEN_PATH=/tmp/flow-b/ipc.token FLOW_B_LOG=b.log \
//!   cargo run -p flow-daemon --example drive_two_daemons
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flow_core::device::{Device, DeviceState};
use flow_core::pairing::PairingStage;
use flow_core::protocol::{InputEvent, KeyboardEvent, Modifier, MouseButton, MouseEvent};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

type Err = Box<dyn std::error::Error + Send + Sync>;
type Res<T> = Result<T, Err>;

/// The receiver-side hop message this driver greps each daemon's log for
/// to confirm an injected event actually landed on the far machine.
const INJECTED_MARKER: &str = "event injected into this machine";

#[tokio::main]
async fn main() {
    match run().await {
        Ok(matrix) => {
            print_matrix(&matrix);
            if matrix.iter().all(|(_, ok, _)| *ok) {
                println!("\nALL CHECKS PASSED");
            } else {
                println!("\nSOME CHECKS FAILED — see the flow::hop logs on both daemons");
                std::process::exit(1);
            }
        }
        Err(err) => {
            eprintln!("driver aborted: {err}");
            std::process::exit(2);
        }
    }
}

async fn run() -> Res<Vec<(String, bool, String)>> {
    let a_port: u16 = env_or("FLOW_A_IPC_PORT", "47823").parse()?;
    let b_port: u16 = env_or("FLOW_B_IPC_PORT", "47833").parse()?;
    let a_token = read_token("FLOW_A_TOKEN_PATH", None)?;
    let b_token = read_token("FLOW_B_TOKEN_PATH", None)?;
    let a_log = std::env::var_os("FLOW_A_LOG").map(PathBuf::from);
    let b_log = std::env::var_os("FLOW_B_LOG").map(PathBuf::from);

    println!("connecting to daemon A on 127.0.0.1:{a_port} …");
    let a = IpcClient::connect("A", a_port, &a_token).await?;
    println!("connecting to daemon B on 127.0.0.1:{b_port} …");
    let b = IpcClient::connect("B", b_port, &b_token).await?;

    let mut matrix: Vec<(String, bool, String)> = Vec::new();

    // ── 1. Pair A → B ────────────────────────────────────────────────
    println!("\n[1] pairing A ↔ B");
    a.command("start_pairing", Value::Null).await?;
    a.wait_until(
        "B appears as a candidate on A",
        Duration::from_secs(20),
        |s| !s.candidate_ids().is_empty(),
    )
    .await?;
    let candidate_id = a
        .state()
        .candidate_ids()
        .into_iter()
        .next()
        .ok_or("no candidate id")?;
    a.command(
        "pair_with_candidate",
        json!({ "candidate_id": candidate_id }),
    )
    .await?;

    a.wait_until("A shows a paired peer", Duration::from_secs(20), |s| {
        s.paired_peer().is_some()
    })
    .await?;
    b.wait_until("B shows a paired peer", Duration::from_secs(20), |s| {
        s.paired_peer().is_some()
    })
    .await?;
    let b_id = a
        .state()
        .paired_peer()
        .ok_or("no paired peer on A")?
        .id
        .0
        .clone();
    let a_id = b
        .state()
        .paired_peer()
        .ok_or("no paired peer on B")?
        .id
        .0
        .clone();
    println!("    paired: A sees B as {b_id}, B sees A as {a_id}");
    matrix.push(("pair A↔B".into(), true, format!("A→{b_id}, B→{a_id}")));

    // Give discovery's redial a beat to bring the streaming pipeline up.
    wait_for_pipeline(&a_log, &b_log).await;

    // ── 2. Control A → B ─────────────────────────────────────────────
    println!("\n[2] control A → B  (make B active on A, inject on A)");
    let (ok, detail) = drive_direction(&a, &b_id, b_log.as_deref(), "A→B").await?;
    matrix.push(("keys+mouse A→B".into(), ok, detail));

    // ── 3. Graceful switch: control back to A ────────────────────────
    println!("\n[3] graceful switch  (make A active again; injected events must stop reaching B)");
    let before = count_marker(b_log.as_deref(), INJECTED_MARKER);
    a.command("switch_active_device", json!({ "device_id": "d1" }))
        .await?;
    a.wait_until("A is active again on A", Duration::from_secs(10), |s| {
        s.device("d1")
            .map(|d| d.state == DeviceState::Active)
            .unwrap_or(false)
    })
    .await?;
    tokio::time::sleep(Duration::from_millis(600)).await;
    for ev in [key("Escape", true), key("Escape", false)] {
        a.command("debug_inject_input", serde_json::to_value(&ev)?)
            .await?;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let after = count_marker(b_log.as_deref(), INJECTED_MARKER);
    let ok = after == before;
    println!("    B injected-event count: before={before} after={after}");
    matrix.push((
        "graceful switch stops flow".into(),
        ok,
        format!("B injections unchanged at {before}"),
    ));

    // ── 4. Control B → A ─────────────────────────────────────────────
    println!("\n[4] control B → A  (make A active on B, inject on B)");
    let (ok, detail) = drive_direction(&b, &a_id, a_log.as_deref(), "B→A").await?;
    matrix.push(("keys+mouse B→A".into(), ok, detail));

    Ok(matrix)
}

/// Switch `target_id` active on `driver`, inject the hardcoded key
/// sequence + mouse path, and confirm they reached the far daemon by
/// grepping `peer_log` for the injection marker.
async fn drive_direction(
    driver: &IpcClient,
    target_id: &str,
    peer_log: Option<&Path>,
    label: &str,
) -> Res<(bool, String)> {
    driver
        .command("switch_active_device", json!({ "device_id": target_id }))
        .await?;
    driver
        .wait_until(
            &format!("{target_id} is Active on {}", driver.name),
            Duration::from_secs(10),
            |s| {
                s.device(target_id)
                    .map(|d| d.state == DeviceState::Active)
                    .unwrap_or(false)
            },
        )
        .await?;
    // let run_paired_connection observe the switch
    tokio::time::sleep(Duration::from_millis(700)).await;

    let events = hardcoded_events();
    let before = count_marker(peer_log, INJECTED_MARKER);
    for ev in &events {
        driver
            .command("debug_inject_input", serde_json::to_value(ev)?)
            .await?;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(800)).await;
    let after = count_marker(peer_log, INJECTED_MARKER);
    let landed = after.saturating_sub(before);

    let detail = match peer_log {
        Some(p) => format!(
            "{label}: sent {}, {} landed on the peer (marker count {before}→{after} in {})",
            events.len(),
            landed,
            p.display()
        ),
        None => format!(
            "{label}: sent {} events; no peer log given, check its flow::hop for `{INJECTED_MARKER}`",
            events.len()
        ),
    };
    println!("    {detail}");
    let ok = peer_log.is_none() || landed >= events.len();
    Ok((ok, detail))
}

/// The fixed test vector: type "flow", move the mouse in a small square,
/// left-click. 8 key events + 4 moves + 2 button events = 14.
fn hardcoded_events() -> Vec<InputEvent> {
    let mut v = Vec::new();
    for ch in ["f", "l", "o", "w"] {
        v.push(key(ch, true));
        v.push(key(ch, false));
    }
    for (dx, dy) in [(20, 0), (0, 20), (-20, 0), (0, -20)] {
        v.push(InputEvent::Mouse(MouseEvent::Move {
            dx,
            dy,
            timestamp_ms: 0,
        }));
    }
    v.push(InputEvent::Mouse(MouseEvent::ButtonDown {
        button: MouseButton::Left,
        timestamp_ms: 0,
    }));
    v.push(InputEvent::Mouse(MouseEvent::ButtonUp {
        button: MouseButton::Left,
        timestamp_ms: 0,
    }));
    v
}

fn key(k: &str, down: bool) -> InputEvent {
    let (key, modifiers, timestamp_ms) = (k.to_string(), Vec::<Modifier>::new(), 0);
    InputEvent::Keyboard(if down {
        KeyboardEvent::KeyDown {
            key,
            modifiers,
            timestamp_ms,
        }
    } else {
        KeyboardEvent::KeyUp {
            key,
            modifiers,
            timestamp_ms,
        }
    })
}

/// Poll both logs (if given) for `pipeline_up` before starting to inject;
/// otherwise just wait a fixed grace period.
async fn wait_for_pipeline(a_log: &Option<PathBuf>, b_log: &Option<PathBuf>) {
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let up = [a_log, b_log].iter().any(|l| {
            l.as_deref()
                .map(|p| count_marker(Some(p), "input-streaming pipeline starting") > 0)
                .unwrap_or(false)
        });
        if up {
            println!("    streaming pipeline is up");
            return;
        }
        if Instant::now() > deadline {
            println!("    (no `pipeline_up` seen in the logs yet — continuing anyway)");
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn count_marker(log: Option<&Path>, needle: &str) -> usize {
    let Some(path) = log else { return 0 };
    std::fs::read_to_string(path)
        .map(|s| s.lines().filter(|l| l.contains(needle)).count())
        .unwrap_or(0)
}

fn print_matrix(matrix: &[(String, bool, String)]) {
    println!("\n──────────────────────────── RESULTS ────────────────────────────");
    for (name, ok, detail) in matrix {
        println!(
            "  [{}] {:<28} {}",
            if *ok { "PASS" } else { "FAIL" },
            name,
            detail
        );
    }
    println!("────────────────────────────────────────────────────────────────");
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn read_token(env_key: &str, fallback: Option<PathBuf>) -> Res<String> {
    let path = std::env::var_os(env_key)
        .map(PathBuf::from)
        .or(fallback)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|home| PathBuf::from(home).join(".flow").join("ipc.token"))
        })
        .ok_or("cannot locate an IPC token path")?;
    let token = std::fs::read_to_string(&path)
        .map_err(|e| format!("reading token {}: {e}", path.display()))?;
    Ok(token.trim().to_string())
}

// ─── minimal IPC client ─────────────────────────────────────────────

#[derive(Default)]
struct ClientState {
    devices: Vec<Device>,
    pairing_stage: Option<PairingStage>,
    candidate_ids: Vec<String>,
}

impl ClientState {
    fn device(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.id.0 == id)
    }
    /// The first non-local device whose id looks like a proven/claimed
    /// public key (`pk:` prefix) — i.e. an actually-paired peer.
    fn paired_peer(&self) -> Option<&Device> {
        self.devices
            .iter()
            .find(|d| d.id.0 != "d1" && d.id.0.starts_with("pk:"))
    }
    fn candidate_ids(&self) -> Vec<String> {
        self.candidate_ids.clone()
    }
}

struct IpcClient {
    name: String,
    cmd_tx: mpsc::UnboundedSender<(Value, oneshot::Sender<Value>)>,
    state: Arc<Mutex<ClientState>>,
}

impl IpcClient {
    async fn connect(name: &str, port: u16, token: &str) -> Res<Self> {
        let mut request = format!("ws://127.0.0.1:{port}").into_client_request()?;
        request
            .headers_mut()
            .insert("sec-websocket-protocol", token.parse()?);
        let (ws, _) = tokio_tungstenite::connect_async(request).await?;
        let (mut sink, mut source) = ws.split();

        let state = Arc::new(Mutex::new(ClientState::default()));
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<(Value, oneshot::Sender<Value>)>();
        let reader_state = Arc::clone(&state);

        tokio::spawn(async move {
            let mut pending: HashMap<String, oneshot::Sender<Value>> = HashMap::new();
            loop {
                tokio::select! {
                    Some((req, ack)) = cmd_rx.recv() => {
                        let id = req["id"].as_str().unwrap_or_default().to_string();
                        pending.insert(id, ack);
                        if sink.send(Message::Text(req.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    msg = source.next() => {
                        let Some(Ok(Message::Text(text))) = msg else {
                            if msg.is_none() { break; }
                            continue;
                        };
                        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                        if let Some(event) = v["event"].as_str() {
                            apply_event(&reader_state, event, &v["payload"]);
                        } else if let Some(id) = v["id"].as_str() {
                            if let Some(ack) = pending.remove(id) {
                                let _ = ack.send(v);
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            name: name.to_string(),
            cmd_tx,
            state,
        })
    }

    fn state(&self) -> ClientState {
        let s = self.state.lock().unwrap();
        ClientState {
            devices: s.devices.clone(),
            pairing_stage: s.pairing_stage,
            candidate_ids: s.candidate_ids.clone(),
        }
    }

    async fn command(&self, command: &str, payload: Value) -> Res<()> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = format!(
            "drv-{}",
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let req = json!({ "id": id, "command": command, "payload": payload });
        let (tx, rx) = oneshot::channel();
        self.cmd_tx.send((req, tx)).map_err(|_| "ipc reader gone")?;
        let resp = tokio::time::timeout(Duration::from_secs(10), rx).await??;
        if resp["ok"] == Value::Bool(true) {
            Ok(())
        } else {
            Err(format!("{} {command} → {}", self.name, resp).into())
        }
    }

    async fn wait_until(
        &self,
        what: &str,
        timeout: Duration,
        pred: impl Fn(&ClientState) -> bool,
    ) -> Res<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if pred(&self.state()) {
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err(format!("{} timed out waiting for: {what}", self.name).into());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

fn apply_event(state: &Arc<Mutex<ClientState>>, event: &str, payload: &Value) {
    let mut s = state.lock().unwrap();
    match event {
        "devices_changed" => {
            if let Ok(devices) = serde_json::from_value::<Vec<Device>>(payload.clone()) {
                s.devices = devices;
            }
        }
        "pairing_session_changed" => {
            if let Ok(stage) = serde_json::from_value::<PairingStage>(payload["stage"].clone()) {
                s.pairing_stage = Some(stage);
            }
            s.candidate_ids = payload["candidates"]
                .as_array()
                .map(|cs| {
                    cs.iter()
                        .filter_map(|c| c["id"].as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
        }
        _ => {}
    }
}
