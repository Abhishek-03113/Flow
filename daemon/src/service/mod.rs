//! The in-memory service state a `DaemonService` (track B2) wraps in
//! watch channels. `load_or_seed` is the load-or-bootstrap step: a fresh
//! (empty) database looks identical to `MockDaemonRepository`'s seed data
//! (`daemon/todos.json` `sharedContractConstants.mockParitySeedData`) —
//! after that first run, whatever was actually persisted comes back
//! instead.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use flow_core::device::{Device, DeviceId, DeviceState, HostOs};
use flow_core::error::FlowError;
use flow_core::link::DaemonLinkState;
use flow_core::pairing::{PairingCandidate, PairingSession, PairingStage};
use flow_core::permission::PermissionStatus;
use flow_core::settings::{FlowSettings, SettingsPatch};
use flow_core::switch_key::SwitchKeyBinding;
use tokio::sync::{watch, Mutex, RwLock};
use tokio::task::JoinHandle;
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

/// `sharedContractConstants.mockParityTimings.pairingSearchToFoundMs`.
const PAIRING_SEARCH_TO_FOUND: Duration = Duration::from_millis(1200);
/// `sharedContractConstants.mockParityTimings.pairingRequestToPairedMs`.
const PAIRING_REQUEST_TO_PAIRED: Duration = Duration::from_millis(1500);
/// `sharedContractConstants.mockParityTimings.pairingTerminalToIdleMs`.
const PAIRING_TERMINAL_TO_IDLE: Duration = Duration::from_millis(1600);

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
#[derive(Clone)]
pub struct DaemonService {
    state: Arc<RwLock<ServiceState>>,
    storage: Storage,
    devices_tx: watch::Sender<Vec<Device>>,
    link_state_tx: watch::Sender<DaemonLinkState>,
    pairing_session_tx: watch::Sender<PairingSession>,
    settings_tx: watch::Sender<FlowSettings>,
    permission_tx: watch::Sender<PermissionStatus>,
    /// The single in-flight pairing timer (search->found,
    /// requesting->paired, or paired->idle), if any. `cancel_pairing`
    /// aborts whatever is here; each timer clears its own slot once it
    /// fires so a later cancel never aborts an already-finished task.
    pairing_timer: Arc<Mutex<Option<JoinHandle<()>>>>,
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
            pairing_timer: Arc::new(Mutex::new(None)),
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

    /// Begins searching for pairing candidates. Errors if a pairing
    /// session is already active anywhere but idle.
    pub async fn start_pairing(&self) -> Result<(), FlowError> {
        {
            let mut state = self.state.write().await;
            if state.pairing_session.stage != PairingStage::Idle {
                return Err(FlowError::PairingInProgress);
            }
            state.pairing_session = PairingSession {
                stage: PairingStage::Searching,
                ..PairingSession::idle()
            };
        }
        self.emit_pairing_session().await;

        let service = self.clone();
        let handle = tokio::spawn(async move { service.on_search_elapsed().await });
        self.set_pairing_timer(handle).await;
        Ok(())
    }

    /// Cancels any in-progress pairing session, returning it to idle.
    /// Aborts whatever timer is pending so a stale firing can't resurrect
    /// the cancelled session.
    pub async fn cancel_pairing(&self) -> Result<(), FlowError> {
        {
            let mut state = self.state.write().await;
            if state.pairing_session.stage == PairingStage::Idle {
                return Err(FlowError::PairingNotActive);
            }
            // Flip to idle before aborting the timer: even if the abort
            // request loses the race with an in-flight timer task, that
            // task's own stage guard (checked under this same lock) will
            // see Idle and bail out without mutating state.
            state.pairing_session = PairingSession::idle();
        }
        self.cancel_pending_timer().await;
        self.emit_pairing_session().await;
        Ok(())
    }

    /// Requests pairing with one of the current session's candidates.
    /// Requires the session to be in `Found`.
    pub async fn pair_with_candidate(&self, candidate_id: &str) -> Result<(), FlowError> {
        let candidate = {
            let mut state = self.state.write().await;
            if state.pairing_session.stage != PairingStage::Found {
                return Err(FlowError::PairingNotReady);
            }
            let candidate = state
                .pairing_session
                .candidates
                .iter()
                .find(|c| c.id == candidate_id)
                .cloned()
                .ok_or_else(|| FlowError::CandidateNotFound(candidate_id.to_string()))?;
            state.pairing_session.stage = PairingStage::Requesting;
            state.pairing_session.target_name = Some(candidate.name.clone());
            candidate
        };
        self.emit_pairing_session().await;

        let service = self.clone();
        let handle = tokio::spawn(async move { service.on_pair_request_elapsed(candidate).await });
        self.set_pairing_timer(handle).await;
        Ok(())
    }

    /// `Searching -> Found`, offering whichever of `candidates_pool`
    /// isn't already a known device name — matches
    /// `MockDaemonRepository.startPairing`'s `_later` callback.
    async fn on_search_elapsed(&self) {
        tokio::time::sleep(PAIRING_SEARCH_TO_FOUND).await;
        {
            let mut state = self.state.write().await;
            if state.pairing_session.stage != PairingStage::Searching {
                return;
            }
            let known: HashSet<&str> = state.devices.values().map(|d| d.name.as_str()).collect();
            let candidates: Vec<PairingCandidate> = state
                .candidates_pool
                .iter()
                .filter(|c| !known.contains(c.name.as_str()))
                .cloned()
                .collect();
            state.pairing_session = PairingSession {
                stage: PairingStage::Found,
                candidates,
                target_name: None,
                error: None,
            };
        }
        self.take_pairing_timer().await;
        self.emit_pairing_session().await;
    }

    /// `Requesting -> Paired`: persists the newly paired device and
    /// chains the paired->idle timer, matching
    /// `MockDaemonRepository.pairWithCandidate`'s nested `_later`.
    async fn on_pair_request_elapsed(&self, candidate: PairingCandidate) {
        tokio::time::sleep(PAIRING_REQUEST_TO_PAIRED).await;

        let new_device = {
            let mut state = self.state.write().await;
            if state.pairing_session.stage != PairingStage::Requesting {
                return;
            }
            let device = Device {
                id: DeviceId(candidate.id.clone()),
                name: candidate.name.clone(),
                os: candidate.os,
                state: DeviceState::Inactive,
                last_seen: Utc::now(),
            };
            state.devices.insert(device.id.clone(), device.clone());
            state.pairing_session.stage = PairingStage::Paired;
            device
        };

        DeviceRepo::new(self.storage.clone())
            .upsert(DeviceRecord {
                device: new_device,
                public_key: None,
                removable: true,
            })
            .await;

        self.take_pairing_timer().await;
        self.emit_devices().await;
        self.emit_pairing_session().await;

        let service = self.clone();
        let handle = tokio::spawn(async move { service.on_paired_elapsed().await });
        self.set_pairing_timer(handle).await;
    }

    /// `Paired -> Idle`, automatically, matching
    /// `MockDaemonRepository.pairWithCandidate`'s innermost `_later`.
    async fn on_paired_elapsed(&self) {
        tokio::time::sleep(PAIRING_TERMINAL_TO_IDLE).await;
        {
            let mut state = self.state.write().await;
            if state.pairing_session.stage != PairingStage::Paired {
                return;
            }
            state.pairing_session = PairingSession::idle();
        }
        self.take_pairing_timer().await;
        self.emit_pairing_session().await;
    }

    async fn emit_pairing_session(&self) {
        let session = self.state.read().await.pairing_session.clone();
        self.pairing_session_tx.send_replace(session);
    }

    async fn emit_devices(&self) {
        let devices = devices_list(&*self.state.read().await);
        self.devices_tx.send_replace(devices);
    }

    /// Installs `handle` as the pending pairing timer.
    async fn set_pairing_timer(&self, handle: JoinHandle<()>) {
        *self.pairing_timer.lock().await = Some(handle);
    }

    /// Clears the pending-timer slot without aborting — used by a timer
    /// callback clearing its own now-irrelevant reference once it has
    /// already run to completion.
    async fn take_pairing_timer(&self) -> Option<JoinHandle<()>> {
        self.pairing_timer.lock().await.take()
    }

    /// Clears and aborts whatever timer is pending, if any.
    async fn cancel_pending_timer(&self) {
        if let Some(handle) = self.take_pairing_timer().await {
            handle.abort();
        }
    }

    /// Sets the switch-key binding. Requires at least one key token.
    pub async fn set_switch_key(&self, binding: SwitchKeyBinding) -> Result<(), FlowError> {
        if binding.keys.is_empty() {
            return Err(FlowError::InvalidSwitchKey);
        }
        self.apply_settings_patch(SettingsPatch {
            switch_key: Some(binding),
            ..Default::default()
        })
        .await;
        Ok(())
    }

    /// Merges `patch` into the current settings (only the `Some` fields
    /// change), matching `FlowSettings::apply_patch`.
    pub async fn update_settings(&self, patch: SettingsPatch) -> Result<(), FlowError> {
        self.apply_settings_patch(patch).await;
        Ok(())
    }

    /// Restores [`FlowSettings::defaults`] exactly.
    pub async fn reset_settings(&self) -> Result<(), FlowError> {
        let defaults = FlowSettings::defaults();
        {
            let mut state = self.state.write().await;
            state.settings = defaults.clone();
        }
        SettingsRepo::new(self.storage.clone())
            .save(defaults.clone())
            .await;
        self.settings_tx.send_replace(defaults);
        Ok(())
    }

    async fn apply_settings_patch(&self, patch: SettingsPatch) {
        let settings = {
            let mut state = self.state.write().await;
            state.settings.apply_patch(patch);
            state.settings.clone()
        };
        SettingsRepo::new(self.storage.clone())
            .save(settings.clone())
            .await;
        self.settings_tx.send_replace(settings);
    }

    /// Grants the OS input-capture permission. Matches the mock's
    /// always-succeeds behavior — a real OS-level denial is explicit
    /// future work once track E's platform adapters can report one.
    pub async fn request_permission(&self) -> Result<(), FlowError> {
        let permission = {
            let mut state = self.state.write().await;
            if state.permission.granted {
                return Err(FlowError::PermissionAlreadyGranted);
            }
            state.permission.granted = true;
            state.permission.clone()
        };
        self.permission_tx.send_replace(permission);
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

        assert_eq!(
            *service.watch_link_state().borrow(),
            DaemonLinkState::Connected
        );
        assert_eq!(
            *service.watch_pairing_session().borrow(),
            PairingSession::idle()
        );
        assert_eq!(*service.watch_settings().borrow(), FlowSettings::defaults());
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

    #[tokio::test]
    async fn start_pairing_while_already_pairing_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        service.start_pairing().await.expect("first start_pairing");
        assert_eq!(
            service.start_pairing().await,
            Err(FlowError::PairingInProgress)
        );
    }

    #[tokio::test]
    async fn pair_with_candidate_before_found_or_with_unknown_id_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        assert_eq!(
            service.pair_with_candidate("cand-office-mini").await,
            Err(FlowError::PairingNotReady)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn pair_with_candidate_with_an_unknown_id_once_found_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;
        let mut sessions = service.watch_pairing_session();
        let _ = sessions.borrow_and_update();

        service.start_pairing().await.expect("start pairing");
        sessions.changed().await.expect("searching update");
        sessions.changed().await.expect("found update");
        assert_eq!(sessions.borrow_and_update().stage, PairingStage::Found);

        assert_eq!(
            service.pair_with_candidate("no-such-candidate").await,
            Err(FlowError::CandidateNotFound(
                "no-such-candidate".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn cancel_pairing_when_idle_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        assert_eq!(
            service.cancel_pairing().await,
            Err(FlowError::PairingNotActive)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn full_pairing_flow_reaches_paired_then_returns_to_idle() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;
        let mut sessions = service.watch_pairing_session();
        let mut devices = service.watch_devices();
        let _ = sessions.borrow_and_update();
        let _ = devices.borrow_and_update();

        service.start_pairing().await.expect("start pairing");

        // idle -> searching (emitted synchronously by start_pairing itself)
        sessions.changed().await.expect("searching update");
        assert_eq!(sessions.borrow_and_update().stage, PairingStage::Searching);

        // searching -> found
        sessions.changed().await.expect("found update");
        let found = sessions.borrow_and_update().clone();
        assert_eq!(found.stage, PairingStage::Found);
        assert_eq!(found.candidates.len(), 2);
        let candidate_id = found.candidates[0].id.clone();
        let candidate_name = found.candidates[0].name.clone();

        service
            .pair_with_candidate(&candidate_id)
            .await
            .expect("pair with candidate");

        // found -> requesting (emitted synchronously by pair_with_candidate itself)
        sessions.changed().await.expect("requesting update");
        assert_eq!(sessions.borrow_and_update().stage, PairingStage::Requesting);

        // requesting -> paired, plus the new device joining the list
        sessions.changed().await.expect("paired update");
        let paired = sessions.borrow_and_update().clone();
        assert_eq!(paired.stage, PairingStage::Paired);
        assert_eq!(paired.target_name.as_deref(), Some(candidate_name.as_str()));

        devices.changed().await.expect("devices update");
        let device_list = devices.borrow_and_update().clone();
        assert!(device_list.iter().any(|d| d.id.0 == candidate_id));

        // paired -> idle, automatically
        sessions.changed().await.expect("idle update");
        let idle = sessions.borrow_and_update().clone();
        assert_eq!(idle, PairingSession::idle());
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_mid_timer_leaves_no_orphaned_mutation() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;
        let mut sessions = service.watch_pairing_session();
        let _ = sessions.borrow_and_update();

        service.start_pairing().await.expect("start pairing");
        sessions.changed().await.expect("searching update");
        assert_eq!(sessions.borrow_and_update().stage, PairingStage::Searching);

        service.cancel_pairing().await.expect("cancel pairing");
        assert_eq!(*sessions.borrow(), PairingSession::idle());

        // Advance well past every mock-parity timer; nothing should fire
        // and resurrect the cancelled session.
        tokio::time::advance(PAIRING_SEARCH_TO_FOUND + PAIRING_REQUEST_TO_PAIRED).await;
        tokio::task::yield_now().await;
        assert_eq!(*sessions.borrow(), PairingSession::idle());
    }

    #[tokio::test]
    async fn set_switch_key_with_empty_keys_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        let empty = SwitchKeyBinding {
            label: "Nothing".to_string(),
            keys: Vec::new(),
        };
        assert_eq!(
            service.set_switch_key(empty).await,
            Err(FlowError::InvalidSwitchKey)
        );
    }

    #[tokio::test]
    async fn set_switch_key_updates_only_the_switch_key_field() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        let binding = SwitchKeyBinding {
            label: "Pause".to_string(),
            keys: vec!["Pause".to_string()],
        };
        service
            .set_switch_key(binding.clone())
            .await
            .expect("set switch key");

        let settings = service.watch_settings().borrow().clone();
        assert_eq!(settings.switch_key, binding);
        assert!(settings.share_mouse); // untouched fields stay default
    }

    #[tokio::test]
    async fn update_settings_merges_only_the_given_field() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        service
            .update_settings(SettingsPatch {
                share_mouse: Some(false),
                ..Default::default()
            })
            .await
            .expect("update settings");

        let settings = service.watch_settings().borrow().clone();
        assert!(!settings.share_mouse);
        assert!(settings.share_keyboard);
        assert!(settings.launch_at_login);
    }

    #[tokio::test]
    async fn update_settings_persists_across_a_reload() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage.clone()).await;

        service
            .update_settings(SettingsPatch {
                debug_logging: Some(true),
                ..Default::default()
            })
            .await
            .expect("update settings");

        let reloaded = SettingsRepo::new(storage).load().await;
        assert!(reloaded.debug_logging);
    }

    #[tokio::test]
    async fn reset_settings_restores_defaults_exactly() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        service
            .update_settings(SettingsPatch {
                share_mouse: Some(false),
                debug_logging: Some(true),
                ..Default::default()
            })
            .await
            .expect("update settings");
        service.reset_settings().await.expect("reset settings");

        let settings = service.watch_settings().borrow().clone();
        assert_eq!(settings, FlowSettings::defaults());
    }

    #[tokio::test]
    async fn request_permission_grants_when_not_yet_granted() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        assert!(!service.watch_permission().borrow().granted);
        service
            .request_permission()
            .await
            .expect("grant permission");
        assert!(service.watch_permission().borrow().granted);
    }

    #[tokio::test]
    async fn request_permission_when_already_granted_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        service
            .request_permission()
            .await
            .expect("grant permission");
        assert_eq!(
            service.request_permission().await,
            Err(FlowError::PermissionAlreadyGranted)
        );
    }
}
