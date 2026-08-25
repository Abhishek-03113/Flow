//! Background task that observes `DaemonService`'s watch-channel event
//! bus and diffs each new value against the last-seen one, appending a
//! [`ConnectionHistoryRepo`] row for meaningful transitions — device
//! state changes, a pairing session reaching `paired`/`failed`, and link
//! state changes — without any command handler in B3-B6 needing to
//! remember to log anything explicitly (`daemon/todos.json` task P5).

use flow_core::device::Device;
use flow_core::link::DaemonLinkState;
use flow_core::pairing::{PairingSession, PairingStage};
use tokio::task::JoinHandle;

use super::connection_history_repo::ConnectionHistoryRepo;
use super::Storage;
use crate::service::DaemonService;

/// Device id used for history rows that aren't about one specific
/// device (currently only `link_state_changed`) — the schema's
/// `device_id` column is `NOT NULL`, and there is no per-device
/// dimension to a daemon-wide link-state transition.
const NON_DEVICE_EVENT_ID: &str = "daemon";

/// Spawns the logger, returning its `JoinHandle` so the caller (`main.rs`
/// from track C4 onward) can hold/abort it alongside the IPC listener and
/// hotkey runner.
pub fn spawn(service: &DaemonService, storage: Storage) -> JoinHandle<()> {
    let repo = ConnectionHistoryRepo::new(storage);
    let mut devices_rx = service.watch_devices();
    let mut pairing_rx = service.watch_pairing_session();
    let mut link_rx = service.watch_link_state();

    // Consume the initial replayed value without logging it — only
    // actual transitions after this point are "history".
    let mut last_devices = devices_rx.borrow_and_update().clone();
    let mut last_pairing = pairing_rx.borrow_and_update().clone();
    let mut last_link = *link_rx.borrow_and_update();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                changed = devices_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let new_devices = devices_rx.borrow_and_update().clone();
                    log_device_transitions(&repo, &last_devices, &new_devices).await;
                    last_devices = new_devices;
                }
                changed = pairing_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let new_pairing = pairing_rx.borrow_and_update().clone();
                    // Peek (not consume) the devices channel's current
                    // value directly rather than relying on `last_devices`
                    // — `DaemonService` commits a devices_tx update before
                    // the corresponding pairing_session_tx update within
                    // the same synchronous span, so `.borrow()` here is
                    // guaranteed to already reflect it regardless of which
                    // branch this select! loop happens to service first.
                    let devices_snapshot = devices_rx.borrow().clone();
                    log_pairing_transition(&repo, &last_pairing, &new_pairing, &devices_snapshot)
                        .await;
                    last_pairing = new_pairing;
                }
                changed = link_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let new_link = *link_rx.borrow_and_update();
                    if new_link != last_link {
                        repo.append(
                            NON_DEVICE_EVENT_ID,
                            "link_state_changed",
                            Some(&format!("{new_link:?}")),
                        )
                        .await;
                    }
                    last_link = new_link;
                }
            }
        }
    })
}

async fn log_device_transitions(repo: &ConnectionHistoryRepo, last: &[Device], current: &[Device]) {
    // A device newly active (switch_active_device) — exactly one row per
    // switch, since only the device that *became* active is logged, not
    // the one demoted alongside it.
    for device in current {
        let was_active_before = last
            .iter()
            .find(|d| d.id == device.id)
            .map(|d| d.state)
            .is_some_and(|state| state == flow_core::device::DeviceState::Active);
        if device.state == flow_core::device::DeviceState::Active && !was_active_before {
            repo.append(&device.id.0, "device_activated", None).await;
        }
    }

    // A device that disappeared (remove_device). A device that newly
    // *appeared* is deliberately not logged here — that's `paired`,
    // logged from the pairing_session transition below so a completed
    // pairing produces one row, not two.
    for device in last {
        if !current.iter().any(|d| d.id == device.id) {
            repo.append(&device.id.0, "device_removed", None).await;
        }
    }
}

async fn log_pairing_transition(
    repo: &ConnectionHistoryRepo,
    last: &PairingSession,
    current: &PairingSession,
    devices_snapshot: &[Device],
) {
    if current.stage == PairingStage::Paired && last.stage != PairingStage::Paired {
        let device_id = current
            .target_name
            .as_deref()
            .and_then(|name| devices_snapshot.iter().find(|d| d.name == name))
            .map(|d| d.id.0.clone())
            .unwrap_or_default();
        repo.append(&device_id, "paired", None).await;
    } else if current.stage == PairingStage::Failed && last.stage != PairingStage::Failed {
        let device_id = current.target_name.clone().unwrap_or_default();
        repo.append(&device_id, "pairing_failed", current.error.as_deref())
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn switching_the_active_device_produces_exactly_one_history_row() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage.clone()).await;
        let _logger = spawn(&service, storage.clone());

        service
            .switch_active_device("d2")
            .await
            .expect("switch to d2");
        tokio::task::yield_now().await;

        let repo = ConnectionHistoryRepo::new(storage);
        let history = repo.recent("d2", 10).await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].event_type, "device_activated");
    }

    #[tokio::test(start_paused = true)]
    async fn a_completed_pairing_produces_a_paired_row() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage.clone()).await;
        let mut sessions = service.watch_pairing_session();
        let _ = sessions.borrow_and_update();
        let _logger = spawn(&service, storage.clone());

        service.start_pairing().await.expect("start pairing");
        sessions.changed().await.expect("searching");
        sessions.changed().await.expect("found");
        let found = sessions.borrow_and_update().clone();
        let candidate = found.candidates.first().unwrap().clone();

        service
            .pair_with_candidate(&candidate.id)
            .await
            .expect("pair with candidate");
        sessions.changed().await.expect("requesting");
        sessions.changed().await.expect("paired");
        tokio::task::yield_now().await;

        let repo = ConnectionHistoryRepo::new(storage);
        let history = repo.recent(&candidate.id, 10).await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].event_type, "paired");
    }

    #[tokio::test(start_paused = true)]
    async fn removing_a_device_produces_a_removed_row() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage.clone()).await;
        let _logger = spawn(&service, storage.clone());

        service.remove_device("d3").await.expect("remove d3");
        tokio::task::yield_now().await;

        let repo = ConnectionHistoryRepo::new(storage);
        let history = repo.recent("d3", 10).await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].event_type, "device_removed");
    }
}
