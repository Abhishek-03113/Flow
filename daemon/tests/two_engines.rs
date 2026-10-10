//! Two complete engines in one process, joined over loopback by real
//! discovery, pairing, Noise-less (insecure dev) sessions and the v2
//! protocol, each on a virtual OS. Proves the controller model end to end:
//! what is typed on the controller reaches only the controlled machine
//! while it is the active screen, the hotkey always brings control home,
//! and the controlled machine's own input is never captured or forwarded.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use flow_core::control::{ActiveScreen, Role};
use flow_core::device::{DeviceId, HostOs, ScreenSize};
use flow_core::input::Disposition;
use flow_core::pairing::PairingStage;
use flow_core::protocol::{InputEvent, KeyboardEvent};
use flow_daemon::devmode::SecurityMode;
use flow_daemon::engine::{DiscoveryConfig, Engine, EngineConfig, EngineHandle};
use flow_daemon::virtual_backend::VirtualBackend;

struct Machine {
    engine: EngineHandle,
    backend: Arc<VirtualBackend>,
    _dir: tempfile::TempDir,
}

async fn machine(name: &str, os: HostOs) -> Machine {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(VirtualBackend::new(
        os,
        ScreenSize {
            width: 1920,
            height: 1080,
        },
    ));
    let mut config = EngineConfig::new(dir.path(), Arc::clone(&backend) as _);
    config.device_name = Some(name.to_string());
    config.security = SecurityMode::Insecure;
    config.discovery = Some(DiscoveryConfig {
        port: 0,
        targets: Some(Vec::new()),
        announce_interval: Duration::from_millis(150),
    });
    let engine = Engine::start(config).await.expect("engine starts");
    Machine {
        engine,
        backend,
        _dir: dir,
    }
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

fn press(key: &str) -> InputEvent {
    InputEvent::Keyboard(KeyboardEvent::KeyDown {
        key: key.to_string(),
        modifiers: vec![],
        timestamp_ms: 0,
    })
}

fn release(key: &str) -> InputEvent {
    InputEvent::Keyboard(KeyboardEvent::KeyUp {
        key: key.to_string(),
        modifiers: vec![],
        timestamp_ms: 0,
    })
}

/// Pairs `a` with `b` through the real discovery + pairing flow and waits
/// until both ends have a live link.
async fn pair_and_link(a: &Machine, b: &Machine) -> DeviceId {
    let loopback = |engine: &EngineHandle| {
        SocketAddr::new(
            Ipv4Addr::LOCALHOST.into(),
            engine.discovery_port().expect("discovery port"),
        )
    };
    a.engine.add_discovery_target(loopback(&b.engine));
    b.engine.add_discovery_target(loopback(&a.engine));

    let service = a.engine.service();
    service.start_pairing().await.expect("start pairing");
    let mut session = service.watch_pairing_session();
    let candidate = loop {
        let current = session.borrow_and_update().clone();
        if current.stage == PairingStage::Found {
            break current.candidates[0].id.clone();
        }
        tokio::time::timeout(Duration::from_secs(10), session.changed())
            .await
            .expect("a candidate is found")
            .unwrap();
    };
    service
        .pair_with_candidate(&candidate)
        .await
        .expect("pair with the candidate");

    eventually("both ends have a live link", || {
        a.engine.service().router().peer_ids().len() == 1
            && b.engine
                .service()
                .watch_control_state()
                .borrow()
                .controller_device_id
                .is_some()
    })
    .await;
    a.engine.service().router().peer_ids().remove(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_controller_drives_the_controlled_machine_and_nothing_leaks_either_way() {
    let windows = machine("Windows PC", HostOs::Windows).await;
    let mac = machine("MacBook", HostOs::Macos).await;
    mac.engine
        .service()
        .set_role(Role::Controlled)
        .await
        .expect("mac is controlled");
    let mac_id = pair_and_link(&windows, &mac).await;

    // The controlled machine never captures; only the controller does.
    assert!(windows.backend.is_capturing());
    assert!(!mac.backend.is_capturing());

    // Local typing stays local.
    assert_eq!(windows.backend.feed(press("L")), Some(Disposition::Pass));
    assert_eq!(windows.backend.feed(release("L")), Some(Disposition::Pass));

    // Switch to the Mac: input is injected there and hidden locally.
    windows
        .engine
        .service()
        .switch_to(&mac_id.0)
        .await
        .expect("switch to the mac");
    eventually("the mac is entered", || {
        mac.engine.service().watch_control_state().borrow().entered
    })
    .await;
    assert!(windows.backend.is_suppressed());
    assert_eq!(windows.backend.feed(press("R")), Some(Disposition::Swallow));
    assert_eq!(
        windows.backend.feed(release("R")),
        Some(Disposition::Swallow)
    );
    eventually("R typed on the mac", || {
        mac.backend.injected().contains(&release("R"))
    })
    .await;
    assert_eq!(
        windows.backend.delivered_locally(),
        vec![press("L"), release("L")],
        "nothing typed while remote reached the controller's own apps"
    );

    // The Mac's own physical input is never forwarded anywhere.
    assert_eq!(mac.backend.feed(press("M")), None);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!windows.backend.injected().contains(&press("M")));
    assert!(!windows.backend.delivered_locally().contains(&press("M")));

    // The hotkey works while suppressed and the chord reaches nobody.
    assert_eq!(
        windows.backend.feed(press("SCROLLLOCK")),
        Some(Disposition::Swallow)
    );
    assert_eq!(
        windows.backend.feed(release("SCROLLLOCK")),
        Some(Disposition::Swallow)
    );
    assert_eq!(
        windows.engine.service().router().active(),
        ActiveScreen::Local
    );
    assert!(!windows.backend.is_suppressed());
    eventually("the mac is left", || {
        !mac.engine.service().watch_control_state().borrow().entered
    })
    .await;
    assert_eq!(windows.backend.feed(press("H")), Some(Disposition::Pass));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!mac.backend.injected().contains(&press("H")));
    assert!(!mac.backend.injected().contains(&press("SCROLLLOCK")));
    assert!(!windows
        .backend
        .delivered_locally()
        .contains(&press("SCROLLLOCK")));

    windows.engine.shutdown();
    mac.engine.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_controlled_machine_can_ask_for_control_back() {
    let windows = machine("Windows PC", HostOs::Windows).await;
    let mac = machine("MacBook", HostOs::Macos).await;
    mac.engine
        .service()
        .set_role(Role::Controlled)
        .await
        .unwrap();
    let mac_id = pair_and_link(&windows, &mac).await;

    windows.engine.service().switch_to(&mac_id.0).await.unwrap();
    eventually("entered", || {
        mac.engine.service().watch_control_state().borrow().entered
    })
    .await;

    mac.engine
        .service()
        .release_control()
        .await
        .expect("release");

    eventually("the controller is home", || {
        windows.engine.service().router().active() == ActiveScreen::Local
    })
    .await;
    assert!(!windows.backend.is_suppressed());

    windows.engine.shutdown();
    mac.engine.shutdown();
}
