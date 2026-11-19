//! Expectations from `docs/redesign/flows-v3.md`, tested under the network
//! conditions real computers meet rather than the ones loopback gives for
//! free: broadcast that only gets through one way, two computers with the
//! same name, a computer that leaves.
//!
//! Every engine here gets an explicit target list (never real broadcast),
//! so a condition is modelled by who announces to whom.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use flow_core::control::Role;
use flow_core::device::{HostOs, ScreenSize};
use flow_core::link::DaemonLinkState;
use flow_core::pairing::{PairingCandidate, PairingStage};
use flow_core::settings::SettingsPatch;
use flow_daemon::devmode::SecurityMode;
use flow_daemon::engine::{DiscoveryConfig, Engine, EngineConfig, EngineHandle};
use flow_daemon::virtual_backend::VirtualBackend;

struct Machine {
    engine: Option<EngineHandle>,
    name: String,
    os: HostOs,
    dir: tempfile::TempDir,
}

impl Machine {
    fn engine(&self) -> &EngineHandle {
        self.engine.as_ref().expect("engine running")
    }
}

async fn machine(name: &str, os: HostOs) -> Machine {
    let dir = tempfile::tempdir().unwrap();
    let engine = start(&dir, name, os, 0, true).await;
    Machine {
        engine: Some(engine),
        name: name.to_string(),
        os,
        dir,
    }
}

/// Starts an engine on `dir`. `discovery: false` runs it with no UDP at
/// all: a network that carries no broadcast, or a firewall that drops it.
async fn start(
    dir: &tempfile::TempDir,
    name: &str,
    os: HostOs,
    peer_port: u16,
    discovery: bool,
) -> EngineHandle {
    let backend = Arc::new(VirtualBackend::new(
        os,
        ScreenSize {
            width: 1920,
            height: 1080,
        },
    ));
    let mut config = EngineConfig::new(dir.path(), backend as _);
    config.device_name = Some(name.to_string());
    config.security = SecurityMode::Insecure;
    config.peer_listen_port = peer_port;
    config.discovery = discovery.then(|| DiscoveryConfig {
        port: 0,
        targets: Some(Vec::new()),
        announce_interval: Duration::from_millis(150),
        answer_announces: true,
    });
    Engine::start(config).await.expect("engine starts")
}

/// Stops `m` and starts it again on the same data and peer port, with no
/// discovery at all.
async fn restart_without_discovery(m: &mut Machine) {
    let port = m.engine().peer_port().expect("peer port");
    m.engine.take().expect("running").shutdown();
    for _ in 0..100 {
        // The old listener closes as its task winds down.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let engine = start(&m.dir, &m.name, m.os, port, false).await;
        if engine.peer_port() == Some(port) {
            m.engine = Some(engine);
            return;
        }
        engine.shutdown();
    }
    panic!("peer port {port} never became free after shutdown");
}

async fn eventually<F: Fn() -> bool>(what: &str, condition: F) {
    for _ in 0..1000 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}");
}

fn linked(controller: &Machine, controlled: &Machine) -> bool {
    controller.engine().service().router().peer_ids().len() == 1
        && controlled
            .engine()
            .service()
            .watch_control_state()
            .borrow()
            .controller_device_id
            .is_some()
}

/// Pairs over discovery (the one moment broadcast is assumed to work) and
/// waits for the link.
async fn pair(controller: &Machine, controlled: &Machine) {
    controlled
        .engine()
        .service()
        .set_role(Role::Controlled)
        .await
        .expect("controlled role");
    controller
        .engine()
        .add_discovery_target(discovery_addr(controlled));
    let listed = candidates(controller, 1).await;
    controller
        .engine()
        .service()
        .pair_with_candidate(&listed[0].id)
        .await
        .expect("pair");
    eventually("the first link", || linked(controller, controlled)).await;
}

fn discovery_addr(m: &Machine) -> SocketAddr {
    SocketAddr::new(
        Ipv4Addr::LOCALHOST.into(),
        m.engine().discovery_port().expect("discovery port"),
    )
}

/// Opens pairing on `m` and waits until `want` candidates are listed.
async fn candidates(m: &Machine, want: usize) -> Vec<PairingCandidate> {
    let service = m.engine().service();
    service.start_pairing().await.expect("start pairing");
    let mut session = service.watch_pairing_session();
    loop {
        let current = session.borrow_and_update().clone();
        if current.stage == PairingStage::Found && current.candidates.len() >= want {
            return current.candidates;
        }
        tokio::time::timeout(Duration::from_secs(10), session.changed())
            .await
            .unwrap_or_else(|_| panic!("{want} candidate(s) listed; have {:?}", current.candidates))
            .unwrap();
    }
}

/// E3/E7: a firewall or network that lets broadcast through in only one
/// direction must still let both computers find each other.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_way_broadcast_is_enough_for_both_sides_to_find_each_other() {
    let pc = machine("Windows PC", HostOs::Windows).await;
    let mac = machine("MacBook", HostOs::Macos).await;
    mac.engine()
        .service()
        .set_role(Role::Controlled)
        .await
        .expect("mac is controlled");

    // Only the PC's announces reach the Mac; the Mac's never reach the PC.
    pc.engine().add_discovery_target(discovery_addr(&mac));

    let seen_by_mac = candidates(&mac, 1).await;
    assert_eq!(seen_by_mac[0].name, "Windows PC");
    assert_eq!(seen_by_mac[0].role, Some(Role::Controller));

    // The PC learns the Mac only from the Mac's unicast answer.
    let seen_by_pc = candidates(&pc, 1).await;
    assert_eq!(seen_by_pc[0].name, "MacBook");
    assert_eq!(seen_by_pc[0].role, Some(Role::Controlled));
}

/// E3: names are labels. Two computers called the same are two candidates.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_computers_with_the_same_name_are_both_offered() {
    let desk = machine("Desk PC", HostOs::Windows).await;
    let first = machine("MacBook Pro", HostOs::Macos).await;
    let second = machine("MacBook Pro", HostOs::Macos).await;
    first.engine().add_discovery_target(discovery_addr(&desk));
    second.engine().add_discovery_target(discovery_addr(&desk));

    let listed = candidates(&desk, 2).await;
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().all(|c| c.name == "MacBook Pro"));
    assert_ne!(listed[0].id, listed[1].id);
}

/// E7: after pairing, the computers reconnect by themselves even when the
/// network never carries another broadcast. The controlled computer
/// restarts first.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paired_computers_reconnect_with_no_broadcast_after_the_controlled_one_restarts() {
    let mut pc = machine("Windows PC", HostOs::Windows).await;
    let mut mac = machine("MacBook", HostOs::Macos).await;
    pair(&pc, &mac).await;

    restart_without_discovery(&mut mac).await;
    eventually("the link after the Mac restarts", || linked(&pc, &mac)).await;

    // And again with both computers restarted, so neither has any
    // discovery running: only stored addresses connect them.
    restart_without_discovery(&mut pc).await;
    eventually("the link after both restart", || linked(&pc, &mac)).await;
}

/// E7: the same with the controller restarting first.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paired_computers_reconnect_with_no_broadcast_after_the_controller_restarts() {
    let mut pc = machine("Windows PC", HostOs::Windows).await;
    let mac = machine("MacBook", HostOs::Macos).await;
    pair(&pc, &mac).await;

    restart_without_discovery(&mut pc).await;
    eventually("the link after the PC restarts", || linked(&pc, &mac)).await;
}

/// E4: on a network that carries no broadcast at all, a person can pair by
/// typing the other computer's address; the computer is then listed under
/// its own name, not the address that was typed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pairing_by_address_works_with_no_discovery_and_shows_the_real_name() {
    let dir_pc = tempfile::tempdir().unwrap();
    let dir_mac = tempfile::tempdir().unwrap();
    let pc = Machine {
        engine: Some(start(&dir_pc, "Windows PC", HostOs::Windows, 0, false).await),
        name: "Windows PC".into(),
        os: HostOs::Windows,
        dir: dir_pc,
    };
    let mac = Machine {
        engine: Some(start(&dir_mac, "MacBook", HostOs::Macos, 0, false).await),
        name: "MacBook".into(),
        os: HostOs::Macos,
        dir: dir_mac,
    };
    mac.engine()
        .service()
        .set_role(Role::Controlled)
        .await
        .expect("controlled role");

    let typed = format!("127.0.0.1:{}", mac.engine().peer_port().expect("peer port"));
    pc.engine()
        .service()
        .pair_with_address(&typed)
        .await
        .expect("pair by address");
    eventually("the link after pairing by address", || linked(&pc, &mac)).await;

    eventually("the Mac listed under its own name", || {
        pc.engine()
            .service()
            .watch_devices()
            .borrow()
            .iter()
            .any(|d| d.name == "MacBook" && d.os == HostOs::Macos)
    })
    .await;
}

/// E9: "Stop Trying" really stops redialling, and Retry dials at once (it
/// used to only change the displayed state and wait for an announce).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_trying_stops_and_retry_dials_immediately() {
    let pc = machine("Windows PC", HostOs::Windows).await;
    let mut mac = machine("MacBook", HostOs::Macos).await;
    pair(&pc, &mac).await;
    // Only the PC may dial, so the test sees the PC's behaviour alone.
    mac.engine()
        .service()
        .update_settings(SettingsPatch {
            auto_connect_paired_devices: Some(false),
            ..SettingsPatch::default()
        })
        .await
        .expect("mac stops dialling");

    // The Mac goes away; the PC starts reconnecting, and the person stops it.
    let port = mac.engine().peer_port().expect("peer port");
    mac.engine.take().expect("running").shutdown();
    let pc_link = pc.engine().service().watch_link_state();
    eventually("the PC notices the Mac is gone", || {
        *pc_link.borrow() == DaemonLinkState::Reconnecting
    })
    .await;
    pc.engine()
        .service()
        .cancel_reconnect()
        .await
        .expect("stop trying");

    // The Mac comes back (no discovery). The PC must not dial on its own.
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let engine = start(&mac.dir, &mac.name, mac.os, port, false).await;
        if engine.peer_port() == Some(port) {
            mac.engine = Some(engine);
            break;
        }
        engine.shutdown();
    }
    assert!(mac.engine.is_some(), "the Mac restarted on its port");
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(!linked(&pc, &mac), "no redial after Stop Trying");

    // Retry: connected well within the shortest backoff step plus a dial.
    let pressed = std::time::Instant::now();
    pc.engine()
        .service()
        .retry_connection()
        .await
        .expect("retry");
    eventually("the link after Retry", || linked(&pc, &mac)).await;
    assert!(
        pressed.elapsed() < Duration::from_secs(3),
        "Retry took {:?}",
        pressed.elapsed()
    );
}
