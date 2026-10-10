//! Two services joined by an in-memory channel: role negotiation, the
//! controller driving a controlled device, and every way a link ends.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use flow_core::channel::{Channel, ChannelError, ChannelKind, ChannelMessage};
use flow_core::control::{ActiveScreen, Role};
use flow_core::device::{DeviceId, DeviceState, HostOs, ScreenSize};
use flow_core::protocol::{InputEvent, KeyboardEvent};
use flow_daemon::channel::memory;
use flow_daemon::link::{run_link, LinkEnv, LinkTiming};
use flow_daemon::service::DaemonService;
use flow_daemon::storage::Storage;
use flow_daemon::virtual_backend::VirtualBackend;

const SCREEN: ScreenSize = ScreenSize {
    width: 1000,
    height: 600,
};

struct Side {
    service: Arc<DaemonService>,
    backend: Arc<VirtualBackend>,
    env: LinkEnv,
}

async fn side(os: HostOs, role: Role, timing: LinkTiming) -> Side {
    let storage = Storage::open_in_memory().await.expect("db");
    let service = Arc::new(DaemonService::new_seeded_for_test(storage).await);
    service.set_role(role).await.expect("role");
    let backend = Arc::new(VirtualBackend::new(os, SCREEN));
    let env = LinkEnv {
        service: Arc::clone(&service),
        backend: Arc::clone(&backend) as _,
        timing,
    };
    Side {
        service,
        backend,
        env,
    }
}

fn fast() -> LinkTiming {
    LinkTiming {
        heartbeat_interval: Duration::from_millis(30),
        liveness_timeout: Duration::from_millis(200),
        hello_timeout: Duration::from_millis(500),
    }
}

fn peer() -> DeviceId {
    // Both seeded services list a device with this id.
    DeviceId("d2".to_string())
}

fn spawn_link(side: &Side, channel: impl Channel + 'static) -> tokio::task::JoinHandle<()> {
    let env = side.env.clone();
    tokio::spawn(async move { run_link(&env, Box::new(channel), peer()).await })
}

async fn eventually<F: Fn() -> bool>(what: &str, condition: F) {
    for _ in 0..400 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
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

fn link_error(side: &Side) -> Option<String> {
    side.service
        .watch_devices()
        .borrow()
        .iter()
        .find(|d| d.id == peer())
        .and_then(|d| d.link_error.clone())
}

#[tokio::test]
async fn two_controllers_are_refused_on_both_sides() {
    let a = side(HostOs::Windows, Role::Controller, fast()).await;
    let b = side(HostOs::Macos, Role::Controller, fast()).await;
    let (ca, cb) = memory::pair();
    spawn_link(&a, ca);
    spawn_link(&b, cb);

    eventually("role_conflict on both", || {
        link_error(&a).as_deref() == Some("role_conflict")
            && link_error(&b).as_deref() == Some("role_conflict")
    })
    .await;
    let state = a.service.watch_devices().borrow().clone();
    assert_eq!(
        state.iter().find(|d| d.id == peer()).unwrap().state,
        DeviceState::Error
    );
}

#[tokio::test]
async fn two_controlled_devices_are_refused() {
    let a = side(HostOs::Windows, Role::Controlled, fast()).await;
    let b = side(HostOs::Macos, Role::Controlled, fast()).await;
    let (ca, cb) = memory::pair();
    spawn_link(&a, ca);
    spawn_link(&b, cb);
    eventually("role_conflict", || {
        link_error(&a).as_deref() == Some("role_conflict")
    })
    .await;
}

#[tokio::test]
async fn a_peer_that_speaks_another_protocol_version_is_refused() {
    let a = side(HostOs::Windows, Role::Controller, fast()).await;
    let (ca, mut old_peer) = memory::pair();
    spawn_link(&a, ca);
    old_peer
        .send(ChannelMessage::Hello {
            protocol_version: 1,
            role: Role::Controlled,
            os: HostOs::Macos,
            screen: SCREEN,
            device_name: "old".to_string(),
        })
        .await
        .unwrap();
    eventually("protocol_mismatch", || {
        link_error(&a).as_deref() == Some("protocol_mismatch")
    })
    .await;
}

/// A v1 peer's first frame isn't even a v2 message.
struct UnparseablePeer;

#[async_trait]
impl Channel for UnparseablePeer {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Tcp
    }
    async fn send(&mut self, _: ChannelMessage) -> Result<(), ChannelError> {
        Ok(())
    }
    async fn recv(&mut self) -> Result<ChannelMessage, ChannelError> {
        Err(ChannelError::Serialization("unknown variant".to_string()))
    }
    async fn close(&mut self) -> Result<(), ChannelError> {
        Ok(())
    }
}

#[tokio::test]
async fn a_v1_peer_whose_frames_do_not_parse_is_refused_clearly() {
    let a = side(HostOs::Windows, Role::Controller, fast()).await;
    spawn_link(&a, UnparseablePeer);
    eventually("protocol_mismatch", || {
        link_error(&a).as_deref() == Some("protocol_mismatch")
    })
    .await;
}

type Task = tokio::task::JoinHandle<()>;

async fn linked_pair() -> (Side, Side, Task, Task) {
    let controller = side(HostOs::Windows, Role::Controller, fast()).await;
    let controlled = side(HostOs::Macos, Role::Controlled, fast()).await;
    controller
        .backend
        .start_capture_for_router(&controller.service);
    let (ca, cb) = memory::pair();
    let controller_task = spawn_link(&controller, ca);
    let controlled_task = spawn_link(&controlled, cb);
    eventually("both links up", || {
        controller.service.router().peer_ids() == [peer()]
            && controlled
                .service
                .watch_control_state()
                .borrow()
                .controller_device_id
                == Some(peer())
    })
    .await;
    (controller, controlled, controller_task, controlled_task)
}

#[tokio::test]
async fn the_controller_drives_the_controlled_device_and_nothing_leaks_locally() {
    let (controller, controlled, _controller_task, _controlled_task) = linked_pair().await;

    // Typing before switching stays local.
    controller.backend.feed(press("A"));
    assert_eq!(controller.backend.delivered_locally(), vec![press("A")]);

    controller
        .service
        .switch_to(&peer().0)
        .await
        .expect("switch");
    eventually("the controlled side is entered", || {
        controlled.service.watch_control_state().borrow().entered
    })
    .await;
    assert!(controller.backend.is_suppressed());

    controller.backend.feed(press("B"));
    eventually("B injected remotely", || {
        controlled.backend.injected().contains(&press("B"))
    })
    .await;
    assert_eq!(
        controller.backend.delivered_locally(),
        vec![press("A")],
        "B must not reach the controller's own apps"
    );

    // The controlled machine's own typing is never captured or forwarded.
    assert!(!controlled.backend.is_capturing());
    assert!(controlled.backend.feed(press("Z")).is_none());

    // The hotkey works while suppressed, and the chord never leaks.
    controller.backend.feed(press("SCROLLLOCK"));
    assert_eq!(controller.service.router().active(), ActiveScreen::Local);
    assert!(!controller.backend.is_suppressed());
    eventually("controlled side left", || {
        !controlled.service.watch_control_state().borrow().entered
    })
    .await;
    assert!(!controller
        .backend
        .delivered_locally()
        .contains(&press("SCROLLLOCK")));
    assert!(!controlled.backend.injected().contains(&press("SCROLLLOCK")));
}

#[tokio::test]
async fn releasing_from_the_controlled_side_brings_the_controller_home() {
    let (controller, controlled, _controller_task, _controlled_task) = linked_pair().await;
    controller.service.switch_to(&peer().0).await.unwrap();
    eventually("entered", || {
        controlled.service.watch_control_state().borrow().entered
    })
    .await;

    controlled.service.release_control().await.expect("release");

    eventually("controller back home", || {
        controller.service.router().active() == ActiveScreen::Local
    })
    .await;
    assert!(!controller.backend.is_suppressed());
}

#[tokio::test]
async fn a_controlled_side_that_loses_its_controller_releases_held_keys() {
    let (controller, controlled, controller_task, controlled_task) = linked_pair().await;
    controller.service.switch_to(&peer().0).await.unwrap();
    controller.backend.feed(press("H"));
    eventually("H injected", || {
        controlled.backend.injected().contains(&press("H"))
    })
    .await;

    // The controller's engine dies without sending a Leave.
    controller_task.abort();
    tokio::time::timeout(Duration::from_secs(3), controlled_task)
        .await
        .expect("the controlled link ends (heartbeat timeout or close)")
        .unwrap();

    assert!(controlled.backend.injected().iter().any(|e| matches!(
        e,
        InputEvent::Keyboard(KeyboardEvent::KeyUp { key, .. }) if key == "H"
    )));
    assert!(!controlled.service.watch_control_state().borrow().entered);
}

#[tokio::test]
async fn a_dead_link_while_remote_returns_the_controller_home() {
    let (controller, controlled, _controller_task, _controlled_task) = linked_pair().await;
    controller.service.switch_to(&peer().0).await.unwrap();
    assert!(controller.backend.is_suppressed());

    // The controlled engine disappears entirely.
    _controlled_task.abort();
    drop(controlled);
    eventually("controller home and unsuppressed", || {
        controller.service.router().active() == ActiveScreen::Local
            && !controller.backend.is_suppressed()
    })
    .await;
}

#[tokio::test]
async fn heartbeat_silence_ends_the_link_and_releases_held_keys() {
    let controlled = side(HostOs::Macos, Role::Controlled, fast()).await;
    let (cb, mut silent_controller) = memory::pair();
    let task = spawn_link(&controlled, cb);
    silent_controller
        .send(ChannelMessage::Hello {
            protocol_version: flow_core::channel::PROTOCOL_VERSION,
            role: Role::Controller,
            os: HostOs::Windows,
            screen: SCREEN,
            device_name: "silent".to_string(),
        })
        .await
        .unwrap();
    silent_controller
        .send(ChannelMessage::Enter {
            seq: 1,
            x: 0,
            y: 0,
            held_modifiers: vec![],
        })
        .await
        .unwrap();
    silent_controller
        .send(ChannelMessage::Input {
            seq: 2,
            event: press("K"),
        })
        .await
        .unwrap();
    eventually("K injected", || {
        controlled.backend.injected().contains(&press("K"))
    })
    .await;

    // The controller stays connected but goes silent.
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .expect("liveness timeout ends the link")
        .unwrap();
    assert!(controlled.backend.injected().iter().any(|e| matches!(
        e,
        InputEvent::Keyboard(KeyboardEvent::KeyUp { key, .. }) if key == "K"
    )));
    drop(silent_controller);
}
