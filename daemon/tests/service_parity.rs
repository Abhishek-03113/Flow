//! Mirrors `flutter/test/data/mock_daemon_repository_test.dart`'s case
//! list against `DaemonService`, so the Rust service is tested against
//! the same behavioral spec the Dart mock already passed
//! (`docs/contracts/README.md` ground rule 2: "MockDaemonRepository must
//! implement the interface exactly, including timing behavior").
//!
//! Case-for-case cross-reference (15 cases in both files) — if a case is
//! added/renamed on one side, update the other:
//!
//! | Dart test name                                                              | Rust test name |
//! |---|---|
//! | watchDevices replays the seeded devices to a new listener                  | `watch_devices_replays_the_seeded_devices_to_a_new_subscriber` |
//! | watchLinkState defaults to connected and reflects debugSetLinkState        | `watch_link_state_defaults_to_connected` (no Rust equivalent to `debugSetLinkState` — it's a Flutter-only dev-harness helper, not part of `DaemonRepository`/`DaemonService`) |
//! | switchActiveDevice moves target to active and demotes the previous active  | `switch_active_device_moves_target_active_and_demotes_the_previous_active` |
//! | switchActiveDevice rejects a disconnected target                           | `switch_active_device_rejects_a_disconnected_target` |
//! | switchActiveDevice rejects an unknown device                               | `switch_active_device_rejects_an_unknown_device` |
//! | removeDevice refuses to remove the local device                            | `remove_device_refuses_to_remove_the_local_device` |
//! | removeDevice drops a non-local device                                      | `remove_device_drops_a_non_local_device` |
//! | pairing runs idle -> searching -> found -> requesting -> paired -> idle    | `pairing_runs_idle_searching_found_requesting_paired_idle` |
//! | cancelPairing resets a searching session to idle                           | `cancel_pairing_resets_a_searching_session_to_idle` |
//! | cancelPairing rejects when nothing is in progress                          | `cancel_pairing_rejects_when_nothing_is_in_progress` |
//! | setSwitchKey updates settings                                              | `set_switch_key_updates_settings` |
//! | resetSettings restores defaults after a change                             | `reset_settings_restores_defaults_after_a_change` |
//! | requestPermission grants and then rejects a second request                 | `request_permission_grants_and_then_rejects_a_second_request` |
//! | retryConnection rejects when the link is not disconnected or error         | `retry_connection_rejects_when_the_link_is_not_disconnected_or_error` |
//! | retryConnection moves a disconnected link through connecting to connected  | `retry_connection_moves_a_disconnected_link_to_connecting` (no Rust equivalent to the mock's auto "connecting -> connected" timer — real recovery is discovery-driven, not this command's own job; see `retry_connection`'s doc comment) |

use flow_core::device::DeviceState;
use flow_core::error::FlowError;
use flow_core::link::DaemonLinkState;
use flow_core::pairing::PairingStage;
use flow_core::switch_key::presets;
use flow_daemon::service::DaemonService;
use flow_daemon::storage::Storage;

async fn service() -> DaemonService {
    let storage = Storage::open_in_memory().await.expect("open in-memory db");
    // The mock-parity fixture: this whole file mirrors
    // mock_daemon_repository_test.dart's case list, so it needs the same
    // seeded devices/candidates/Connected link state that mock ships.
    DaemonService::new_seeded_for_test(storage).await
}

#[tokio::test]
async fn watch_devices_replays_the_seeded_devices_to_a_new_subscriber() {
    let service = service().await;
    let devices = service.watch_devices().borrow().clone();

    assert_eq!(devices.len(), 3);
    assert_eq!(
        devices.iter().find(|d| d.id.0 == "d1").unwrap().state,
        DeviceState::Active
    );
    assert_eq!(
        devices.iter().find(|d| d.id.0 == "d2").unwrap().state,
        DeviceState::Inactive
    );
    assert_eq!(
        devices.iter().find(|d| d.id.0 == "d3").unwrap().state,
        DeviceState::Disconnected
    );
}

#[tokio::test]
async fn watch_link_state_defaults_to_connected() {
    let service = service().await;
    assert_eq!(
        *service.watch_link_state().borrow(),
        DaemonLinkState::Connected
    );
}

#[tokio::test(start_paused = true)]
async fn switch_active_device_moves_target_active_and_demotes_the_previous_active() {
    let service = service().await;

    service
        .switch_active_device("d2")
        .await
        .expect("switch to d2");

    let devices = service.watch_devices().borrow().clone();
    assert_eq!(
        devices.iter().find(|d| d.id.0 == "d2").unwrap().state,
        DeviceState::Active
    );
    assert_eq!(
        devices.iter().find(|d| d.id.0 == "d1").unwrap().state,
        DeviceState::Inactive
    );
}

#[tokio::test]
async fn switch_active_device_rejects_a_disconnected_target() {
    let service = service().await;
    assert_eq!(
        service.switch_active_device("d3").await,
        Err(FlowError::DeviceNotSwitchable(flow_core::device::DeviceId(
            "d3".to_string()
        )))
    );
}

#[tokio::test]
async fn switch_active_device_rejects_an_unknown_device() {
    let service = service().await;
    assert_eq!(
        service.switch_active_device("nope").await,
        Err(FlowError::DeviceNotFound(flow_core::device::DeviceId(
            "nope".to_string()
        )))
    );
}

#[tokio::test]
async fn remove_device_refuses_to_remove_the_local_device() {
    let service = service().await;
    assert_eq!(
        service.remove_device("d1").await,
        Err(FlowError::DeviceNotRemovable(flow_core::device::DeviceId(
            "d1".to_string()
        )))
    );
}

#[tokio::test]
async fn remove_device_drops_a_non_local_device() {
    let service = service().await;
    service.remove_device("d3").await.expect("remove d3");
    let devices = service.watch_devices().borrow().clone();
    assert!(!devices.iter().any(|d| d.id.0 == "d3"));
}

#[tokio::test(start_paused = true)]
async fn pairing_runs_idle_searching_found_requesting_paired_idle() {
    let service = service().await;
    let mut sessions = service.watch_pairing_session();
    let _ = sessions.borrow_and_update();
    assert_eq!(sessions.borrow().stage, PairingStage::Idle);

    service.start_pairing().await.expect("start pairing");
    sessions.changed().await.expect("searching");
    assert_eq!(sessions.borrow_and_update().stage, PairingStage::Searching);

    sessions.changed().await.expect("found");
    let found = sessions.borrow_and_update().clone();
    assert_eq!(found.stage, PairingStage::Found);
    assert!(!found.candidates.is_empty());
    let candidate = found.candidates.first().unwrap().clone();

    service
        .pair_with_candidate(&candidate.id)
        .await
        .expect("pair with candidate");
    sessions.changed().await.expect("requesting");
    assert_eq!(sessions.borrow_and_update().stage, PairingStage::Requesting);

    sessions.changed().await.expect("paired");
    let paired = sessions.borrow_and_update().clone();
    assert_eq!(paired.stage, PairingStage::Paired);
    assert_eq!(paired.target_name.as_deref(), Some(candidate.name.as_str()));

    let devices = service.watch_devices().borrow().clone();
    assert!(devices.iter().any(|d| d.name == candidate.name));

    sessions.changed().await.expect("back to idle");
    assert_eq!(sessions.borrow_and_update().stage, PairingStage::Idle);
}

#[tokio::test(start_paused = true)]
async fn cancel_pairing_resets_a_searching_session_to_idle() {
    let service = service().await;
    service.start_pairing().await.expect("start pairing");
    service.cancel_pairing().await.expect("cancel pairing");
    assert_eq!(
        service.watch_pairing_session().borrow().stage,
        PairingStage::Idle
    );
}

#[tokio::test]
async fn cancel_pairing_rejects_when_nothing_is_in_progress() {
    let service = service().await;
    assert_eq!(
        service.cancel_pairing().await,
        Err(FlowError::PairingNotActive)
    );
}

#[tokio::test]
async fn set_switch_key_updates_settings() {
    let service = service().await;
    let f13 = presets()[2].clone();
    assert_eq!(f13.label, "F13");

    service.set_switch_key(f13).await.expect("set switch key");
    let settings = service.watch_settings().borrow().clone();
    assert_eq!(settings.switch_key.label, "F13");
}

#[tokio::test]
async fn reset_settings_restores_defaults_after_a_change() {
    let service = service().await;
    let pause = presets()[1].clone();
    service.set_switch_key(pause).await.expect("set switch key");
    service.reset_settings().await.expect("reset settings");

    let settings = service.watch_settings().borrow().clone();
    assert_eq!(
        settings.switch_key.label,
        flow_core::switch_key::default_binding().label
    );
}

#[tokio::test]
async fn request_permission_grants_and_then_rejects_a_second_request() {
    let service = service().await;
    assert!(!service.watch_permission().borrow().granted);

    service
        .request_permission()
        .await
        .expect("grant permission");
    assert!(service.watch_permission().borrow().granted);

    assert_eq!(
        service.request_permission().await,
        Err(FlowError::PermissionAlreadyGranted)
    );
}

#[tokio::test]
async fn retry_connection_rejects_when_the_link_is_not_disconnected_or_error() {
    let service = service().await;
    assert_eq!(
        *service.watch_link_state().borrow(),
        DaemonLinkState::Connected
    );

    assert_eq!(
        service.retry_connection().await,
        Err(FlowError::LinkNotRecoverable(DaemonLinkState::Connected))
    );
}

#[tokio::test]
async fn retry_connection_moves_a_disconnected_link_to_connecting() {
    let service = service().await;
    service.set_link_state(DaemonLinkState::Disconnected);

    service.retry_connection().await.expect("retry accepted");

    assert_eq!(
        *service.watch_link_state().borrow(),
        DaemonLinkState::Connecting
    );
}
