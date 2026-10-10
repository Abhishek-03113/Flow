//! The engine starts from a config alone, serves the control contract on
//! an ephemeral loopback port, and several engines coexist in one process.

use std::sync::Arc;

use flow_core::device::{HostOs, ScreenSize};
use flow_daemon::engine::{DiscoveryConfig, Engine, EngineConfig, IpcConfig};
use flow_daemon::virtual_backend::VirtualBackend;
use futures_util::StreamExt;
use serde_json::Value;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

fn config(dir: &std::path::Path, os: HostOs) -> EngineConfig {
    let backend = VirtualBackend::new(
        os,
        ScreenSize {
            width: 1920,
            height: 1080,
        },
    );
    let mut config = EngineConfig::new(dir, Arc::new(backend));
    config.ipc = Some(IpcConfig {
        port: 0,
        token_path: dir.join("ipc.token"),
    });
    config.discovery = Some(DiscoveryConfig {
        port: 0,
        targets: Some(Vec::new()),
    });
    config
}

#[tokio::test]
async fn an_engine_serves_the_initial_state_over_its_own_ports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = Engine::start(config(dir.path(), HostOs::Macos))
        .await
        .expect("start");

    assert!(engine.peer_port().is_some_and(|port| port != 0));
    assert!(engine.discovery_port().is_some_and(|port| port != 0));
    let addr = engine.ipc_addr().expect("ipc address");

    let token = std::fs::read_to_string(dir.path().join("ipc.token")).expect("token written");
    let mut request = format!("ws://{addr}").into_client_request().unwrap();
    request
        .headers_mut()
        .insert("sec-websocket-protocol", token.trim().parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("connect");

    let first = ws.next().await.expect("a frame").expect("frame ok");
    let event: Value = serde_json::from_str(&first.into_text().unwrap()).unwrap();
    assert_eq!(event["event"], "devices_changed");
    assert_eq!(event["payload"][0]["os"], "macos");

    engine.shutdown();
}

#[tokio::test]
async fn two_engines_run_isolated_in_one_process() {
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let a = Engine::start(config(a_dir.path(), HostOs::Windows))
        .await
        .expect("start a");
    let b = Engine::start(config(b_dir.path(), HostOs::Macos))
        .await
        .expect("start b");

    assert_ne!(a.peer_port(), b.peer_port());
    assert_ne!(a.ipc_addr(), b.ipc_addr());
    assert_ne!(
        a.service().local_instance_id(),
        b.service().local_instance_id()
    );
    a.shutdown();
    b.shutdown();
}
