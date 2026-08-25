//! The in-memory service state a `DaemonService` (track B2) wraps in
//! watch channels. `load_or_seed` is the load-or-bootstrap step: a fresh
//! (empty) database looks identical to `MockDaemonRepository`'s seed data
//! (`daemon/todos.json` `sharedContractConstants.mockParitySeedData`) —
//! after that first run, whatever was actually persisted comes back
//! instead.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use flow_core::device::{Device, DeviceId, DeviceState, HostOs};
use flow_core::error::FlowError;
use flow_core::link::DaemonLinkState;
use flow_core::pairing::{PairingCandidate, PairingSession};
use flow_core::permission::PermissionStatus;
use flow_core::settings::FlowSettings;
use tokio::sync::{watch, RwLock};
use tokio::time::Duration;

use crate::storage::device_repo::{DeviceRecord, DeviceRepo};
use crate::storage::settings_repo::SettingsRepo;
use crate::storage::Storage;

/// "This device" — the machine `flow-daemon` itself is running on. Never
/// removable, never offered as a pairing candidate; matches
/// `MockDaemonRepository._localDeviceId`.
pub const LOCAL_DEVICE_ID: &str = "d1";

/// Delay before a `switch_active_device` command takes effect, matching
/// `MockDaemonRepository._switchDebounce`.
const SWITCH_DEBOUNCE: Duration = Duration::from_millis(400);

pub struct ServiceState {
    pub devices: HashMap<DeviceId, Device>,
    pub link_state: DaemonLinkState,
    pub pairing_session: PairingSession,
    pub settings: FlowSettings,
    pub permission: PermissionStatus,
    /// The full pool of discoverable pairing candidates; `start_pairing`
    /// (track B4) offers whichever of these aren't already a known
    /// device name, mirroring `MockDaemonRepository._candidateSeeds`.
    pub candidates_pool: Vec<PairingCandidate>,
}

impl ServiceState {
    /// Loads devices and settings from `storage`, seeding the exact
    /// mock-parity 3-device/2-candidate data only when the database is
    /// empty (first run).
    pub async fn load_or_seed(storage: &Storage) -> Self {
        let settings_repo = SettingsRepo::new(storage.clone());
        let device_repo = DeviceRepo::new(storage.clone());

        let settings = settings_repo.load().await;

        let existing = device_repo.list().await;
        let devices = if existing.is_empty() {
            let seed = seed_device_records();
            for record in &seed {
                device_repo.upsert(record.clone()).await;
            }
            seed.into_iter()
                .map(|record| (record.device.id.clone(), record.device))
                .collect()
        } else {
            existing
                .into_iter()
                .map(|record| (record.device.id.clone(), record.device))
                .collect()
        };

        Self {
            devices,
            link_state: DaemonLinkState::Connected,
            pairing_session: PairingSession::idle(),
            settings,
            permission: PermissionStatus {
                name: "Accessibility access".to_string(),
                granted: false,
            },
            candidates_pool: candidate_seeds(),
        }
    }
}

/// Devices as an ordered list (`data-model.md`'s `Device` wire type is a
/// list, `ServiceState.devices` is a map for O(1) lookup by id) — sorted
/// by id for a deterministic watch value.
fn devices_list(state: &ServiceState) -> Vec<Device> {
    let mut list: Vec<Device> = state.devices.values().cloned().collect();
    list.sort_by(|a, b| a.id.0.cmp(&b.id.0));
    list
}

/// Wraps [`ServiceState`] in a `tokio::sync::watch` channel per slice.
/// `watch::Sender`/`Receiver` natively replay the latest value to a newly
/// subscribing receiver, which is exactly the "connecting... is the
/// initial fetch" semantics `docs/contracts/daemon-ipc.md` requires —
/// the same semantics `MockDaemonRepository`'s `_StateChannel` needed a
/// `Stream.multi` rewrite on the Dart side to get right (an `async*`
/// generator can silently drop an emission racing its first microtask).
pub struct DaemonService {
    state: Arc<RwLock<ServiceState>>,
    storage: Storage,
    devices_tx: watch::Sender<Vec<Device>>,
    link_state_tx: watch::Sender<DaemonLinkState>,
    pairing_session_tx: watch::Sender<PairingSession>,
    settings_tx: watch::Sender<FlowSettings>,
    permission_tx: watch::Sender<PermissionStatus>,
}

impl DaemonService {
    pub async fn new(storage: Storage) -> Self {
        let state = ServiceState::load_or_seed(&storage).await;

        let (devices_tx, _) = watch::channel(devices_list(&state));
        let (link_state_tx, _) = watch::channel(state.link_state);
        let (pairing_session_tx, _) = watch::channel(state.pairing_session.clone());
        let (settings_tx, _) = watch::channel(state.settings.clone());
        let (permission_tx, _) = watch::channel(state.permission.clone());

        Self {
            state: Arc::new(RwLock::new(state)),
            storage,
            devices_tx,
            link_state_tx,
            pairing_session_tx,
            settings_tx,
            permission_tx,
        }
    }

    pub fn watch_devices(&self) -> watch::Receiver<Vec<Device>> {
        self.devices_tx.subscribe()
    }

    pub fn watch_link_state(&self) -> watch::Receiver<DaemonLinkState> {
        self.link_state_tx.subscribe()
    }

    pub fn watch_pairing_session(&self) -> watch::Receiver<PairingSession> {
        self.pairing_session_tx.subscribe()
    }

    pub fn watch_settings(&self) -> watch::Receiver<FlowSettings> {
        self.settings_tx.subscribe()
    }

    pub fn watch_permission(&self) -> watch::Receiver<PermissionStatus> {
        self.permission_tx.subscribe()
    }

    /// Makes `device_id` the active device, moving whichever device was
    /// previously active back to inactive. Matches
    /// `MockDaemonRepository.switchActiveDevice`, including its debounce
    /// delay (`SWITCH_DEBOUNCE` — not part of
    /// `sharedContractConstants.mockParityTimings`, but present in the
    /// Dart mock it mirrors; see this module's `buildNote` in
    /// `daemon/todos.json`).
    pub async fn switch_active_device(&self, device_id: &str) -> Result<(), FlowError> {
        let target_id = DeviceId(device_id.to_string());
        {
            let state = self.state.read().await;
            let target = state
                .devices
                .get(&target_id)
                .ok_or_else(|| FlowError::DeviceNotFound(target_id.clone()))?;
            if target.state != DeviceState::Inactive && target.state != DeviceState::Connected {
                return Err(FlowError::DeviceNotSwitchable(target_id));
            }
        }

        tokio::time::sleep(SWITCH_DEBOUNCE).await;

        let devices = {
            let mut state = self.state.write().await;
            for (id, device) in state.devices.iter_mut() {
                if *id == target_id {
                    device.state = DeviceState::Active;
                    device.last_seen = Utc::now();
                } else if device.state == DeviceState::Active {
                    device.state = DeviceState::Inactive;
                }
            }
            devices_list(&state)
        };

        self.devices_tx.send_replace(devices);
        Ok(())
    }

    /// Removes a paired device. "This device" (`LOCAL_DEVICE_ID`) is
    /// never removable, matching the mock.
    pub async fn remove_device(&self, device_id: &str) -> Result<(), FlowError> {
        if device_id == LOCAL_DEVICE_ID {
            return Err(FlowError::DeviceNotRemovable(DeviceId(
                device_id.to_string(),
            )));
        }

        let target_id = DeviceId(device_id.to_string());
        let devices = {
            let mut state = self.state.write().await;
            if state.devices.remove(&target_id).is_none() {
                return Err(FlowError::DeviceNotFound(target_id));
            }
            devices_list(&state)
        };

        // Removal is durable: a removed device must not reappear from a
        // stale devices-table row after a restart.
        DeviceRepo::new(self.storage.clone())
            .remove(target_id)
            .await;

        self.devices_tx.send_replace(devices);
        Ok(())
    }
}

fn seed_device_records() -> Vec<DeviceRecord> {
    let now = Utc::now();
    vec![
        DeviceRecord {
            device: Device {
                id: DeviceId(LOCAL_DEVICE_ID.to_string()),
                name: "MacBook".to_string(),
                os: HostOs::Macos,
                state: DeviceState::Active,
                last_seen: now,
            },
            public_key: None,
            removable: false,
        },
        DeviceRecord {
            device: Device {
                id: DeviceId("d2".to_string()),
                name: "Work Laptop".to_string(),
                os: HostOs::Windows,
                state: DeviceState::Inactive,
                last_seen: now - ChronoDuration::minutes(2),
            },
            public_key: None,
            removable: true,
        },
        DeviceRecord {
            device: Device {
                id: DeviceId("d3".to_string()),
                name: "Desktop".to_string(),
                os: HostOs::Linux,
                state: DeviceState::Disconnected,
                last_seen: now - ChronoDuration::days(3),
            },
            public_key: None,
            removable: true,
        },
    ]
}

fn candidate_seeds() -> Vec<PairingCandidate> {
    vec![
        PairingCandidate {
            id: "cand-office-mini".to_string(),
            name: "Office Mac Mini".to_string(),
            os: HostOs::Macos,
        },
        PairingCandidate {
            id: "cand-studio-linux".to_string(),
            name: "Studio Linux".to_string(),
            os: HostOs::Linux,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fresh_database_seeds_the_mock_parity_devices() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let state = ServiceState::load_or_seed(&storage).await;

        assert_eq!(state.devices.len(), 3);
        let local = &state.devices[&DeviceId(LOCAL_DEVICE_ID.to_string())];
        assert_eq!(local.name, "MacBook");
        assert_eq!(local.state, DeviceState::Active);

        // The seed's not-removable flag actually landed in storage, since
        // that's where B3's precondition checks will eventually read
        // trust/removability from for anything other than the hardcoded
        // LOCAL_DEVICE_ID fast path.
        let device_repo = DeviceRepo::new(storage);
        let local_record = device_repo
            .find_by_id(DeviceId(LOCAL_DEVICE_ID.to_string()))
            .await
            .expect("local device persisted");
        assert!(!local_record.removable);

        assert_eq!(state.candidates_pool.len(), 2);
        assert_eq!(state.pairing_session, PairingSession::idle());
    }

    #[tokio::test]
    async fn a_previously_persisted_device_list_is_loaded_instead_of_reseeded() {
        let storage = Storage::open_in_memory().await.expect("open db");

        // Simulate a prior run that only ever paired one device.
        let device_repo = DeviceRepo::new(storage.clone());
        device_repo
            .upsert(DeviceRecord {
                device: Device {
                    id: DeviceId("only-device".to_string()),
                    name: "Solo".to_string(),
                    os: HostOs::Linux,
                    state: DeviceState::Active,
                    last_seen: Utc::now(),
                },
                public_key: None,
                removable: true,
            })
            .await;

        let state = ServiceState::load_or_seed(&storage).await;

        assert_eq!(state.devices.len(), 1);
        assert!(state
            .devices
            .contains_key(&DeviceId("only-device".to_string())));
    }

    #[tokio::test]
    async fn a_subscriber_immediately_sees_the_seeded_state_with_no_prior_emit() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        // No emit has happened yet — a fresh subscriber must still see the
        // seeded value, proving late-subscribe replay (the exact bug class
        // the Dart mock's async*-based _StateChannel hit before its
        // Stream.multi rewrite).
        let devices = service.watch_devices().borrow().clone();
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].id, DeviceId("d1".to_string()));

        assert_eq!(*service.watch_link_state().borrow(), DaemonLinkState::Connected);
        assert_eq!(
            *service.watch_pairing_session().borrow(),
            PairingSession::idle()
        );
        assert_eq!(
            *service.watch_settings().borrow(),
            FlowSettings::defaults()
        );
        assert!(!service.watch_permission().borrow().granted);
    }

    #[tokio::test]
    async fn each_subscriber_gets_its_own_receiver_all_seeing_the_same_replayed_value() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        let a = service.watch_devices();
        let b = service.watch_devices();
        assert_eq!(a.borrow().len(), b.borrow().len());
    }

    #[tokio::test(start_paused = true)]
    async fn switching_to_a_missing_or_active_device_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        assert_eq!(
            service.switch_active_device("no-such-device").await,
            Err(FlowError::DeviceNotFound(DeviceId(
                "no-such-device".to_string()
            )))
        );
        // d1 is already active in the seed data.
        assert_eq!(
            service.switch_active_device(LOCAL_DEVICE_ID).await,
            Err(FlowError::DeviceNotSwitchable(DeviceId(
                LOCAL_DEVICE_ID.to_string()
            )))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn switching_moves_exactly_one_device_active_and_the_prior_one_inactive() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;
        let mut watch = service.watch_devices();
        let _ = watch.borrow_and_update(); // consume the initial replay

        // Time is paused; awaiting the internal SWITCH_DEBOUNCE sleep
        // auto-advances the virtual clock since nothing else is runnable.
        service.switch_active_device("d2").await.expect("switch d2");

        assert!(watch.changed().await.is_ok());
        let devices = watch.borrow_and_update().clone();
        let d1 = devices.iter().find(|d| d.id.0 == "d1").unwrap();
        let d2 = devices.iter().find(|d| d.id.0 == "d2").unwrap();
        assert_eq!(d1.state, DeviceState::Inactive);
        assert_eq!(d2.state, DeviceState::Active);
        assert_eq!(
            devices
                .iter()
                .filter(|d| d.state == DeviceState::Active)
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn removing_the_local_device_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        assert_eq!(
            service.remove_device(LOCAL_DEVICE_ID).await,
            Err(FlowError::DeviceNotRemovable(DeviceId(
                LOCAL_DEVICE_ID.to_string()
            )))
        );
    }

    #[tokio::test]
    async fn removing_an_unknown_device_returns_not_found() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        assert_eq!(
            service.remove_device("no-such-device").await,
            Err(FlowError::DeviceNotFound(DeviceId(
                "no-such-device".to_string()
            )))
        );
    }

    #[tokio::test]
    async fn removing_a_device_persists_across_a_reload() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage.clone()).await;

        service.remove_device("d3").await.expect("remove d3");
        let devices = service.watch_devices().borrow().clone();
        assert!(!devices.iter().any(|d| d.id.0 == "d3"));

        // Simulate a restart against the same database.
        let reloaded = ServiceState::load_or_seed(&storage).await;
        assert!(!reloaded.devices.contains_key(&DeviceId("d3".to_string())));
    }
}
