//! The in-memory service state a `DaemonService` (track B2) wraps in
//! watch channels. `ServiceState::from_storage` is the real
//! load-or-bootstrap step a production `flow-daemon` process uses: a
//! fresh (empty) database gets only this machine's own real device
//! record, never fake remote devices or candidates — after the first
//! run, whatever was actually persisted comes back instead.
//! `ServiceState::seeded_for_test` is the mock-parity fixture
//! (`daemon/todos.json` `sharedContractConstants.mockParitySeedData`)
//! tests opt into explicitly instead.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use flow_core::channel::{Channel, ChannelAddress, ChannelError};
use flow_core::device::{Device, DeviceId, DeviceState, HostOs};
use flow_core::error::FlowError;
use flow_core::link::DaemonLinkState;
use flow_core::pairing::{
    IncomingPairingRequest, PairingCandidate, PairingDecision, PairingRequest, PairingSession,
    PairingStage,
};
use flow_core::permission::PermissionStatus;
use flow_core::settings::{FlowSettings, SettingsPatch};
use flow_core::switch_key::SwitchKeyBinding;
use tokio::sync::{oneshot, watch, Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio::time::Duration;

use crate::channel::noise::NoiseChannel;
use crate::channel::{handshake, negotiate};
use crate::discovery::DiscoveredPeer;
use crate::identity::DeviceIdentity;
use crate::pairing_fingerprint::key_fingerprint;
use crate::storage::device_repo::{DeviceRecord, DeviceRepo};
use crate::storage::settings_repo::SettingsRepo;
use crate::storage::Storage;
use crate::trust::TrustGate;

/// Outcome of [`DaemonService::accept_incoming_peer_channel`]: either the
/// connection was a pairing attempt from a peer this daemon didn't yet
/// trust (handled and recorded in place — nothing further for the
/// caller to do), or it's a live, Noise-authenticated connection to an
/// already-paired device, handed back so `main.rs` can run the
/// input-streaming pipeline over it.
pub enum IncomingPeerConnection {
    HandledAsPairing,
    TrustedPeer(Box<dyn Channel>, DeviceId, ConnectionPrecedence),
}

/// Whether a connection should win against a competing one to the same
/// peer — see [`DaemonService::connection_precedence`] for the rule and
/// why one is needed at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionPrecedence {
    /// Keep this connection, dropping any competing one.
    Preferred,
    /// Drop this connection; the peer's competing one is the keeper.
    Redundant,
}

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

/// How long an incoming pairing request waits for the local user's
/// Accept/Reject before the daemon rejects it on their behalf.
const PAIRING_DECISION_TIMEOUT: Duration = Duration::from_secs(30);

/// How long [`DaemonService::dial_if_trusted`] will spend connecting to
/// and handshaking with a discovered peer before giving up. Generous
/// enough for a slow LAN and a Noise `XX` round trip, short enough that
/// a peer which never answers doesn't hold a caller indefinitely.
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// Live peers discovered over a real `Channel` medium
    /// (`note_discovered_peer`, track G7), keyed by `PairingCandidate.id`
    /// alongside where to reach them. Kept separate from
    /// `candidates_pool`: whenever this is non-empty for a given
    /// candidate, `pair_with_candidate` performs a real handshake for it
    /// instead of the timer-only mock flow — and since nothing in this
    /// codebase populates it outside of `note_discovered_peer` being
    /// called explicitly, `B7`'s mock-parity tests (which never call it)
    /// keep exercising the pure mock flow unchanged, with no separate
    /// feature flag needed.
    pub discovered_candidates: HashMap<String, (PairingCandidate, ChannelAddress)>,
    /// The single incoming pairing request currently awaiting a
    /// decision, if any. `None` whenever nothing is pending.
    pub incoming_request: Option<PendingPairingRequest>,
}

/// An incoming pairing request the daemon has surfaced to the UI and is
/// blocking a handshake on. `respond_to_pairing_request` fires
/// `responder`; `accept_pairing_over` owns clearing this slot.
pub struct PendingPairingRequest {
    pub info: IncomingPairingRequest,
    responder: oneshot::Sender<PairingDecision>,
}

impl ServiceState {
    /// Production entry point (`DaemonService::new`'s only caller,
    /// `main.rs`): loads devices and settings from `storage`, seeding
    /// only this machine's own real device record when the database is
    /// empty (first run) — never fake remote devices, never fake pairing
    /// candidates. `link_state` starts `Disconnected`: nothing here has
    /// proven a real peer connection yet, so nothing should claim one.
    /// `main.rs`'s `run_peer_pipeline` is the only place that later calls
    /// `DaemonService::set_link_state(Connected)`, once a real handshake
    /// actually completes.
    pub async fn from_storage(storage: &Storage) -> Self {
        let settings_repo = SettingsRepo::new(storage.clone());
        let device_repo = DeviceRepo::new(storage.clone());

        let settings = settings_repo.load().await;

        let existing = device_repo.list().await;
        let mut devices: HashMap<DeviceId, Device> = if existing.is_empty() {
            let local = real_local_device_record();
            device_repo.upsert(local.clone()).await;
            HashMap::from([(local.device.id.clone(), local.device)])
        } else {
            existing
                .into_iter()
                .map(|record| (record.device.id.clone(), record.device))
                .collect()
        };
        restore_local_device_active(&mut devices);

        Self {
            devices,
            link_state: DaemonLinkState::Disconnected,
            pairing_session: PairingSession::idle(),
            settings,
            permission: default_permission(),
            candidates_pool: Vec::new(),
            discovered_candidates: HashMap::new(),
            incoming_request: None,
        }
    }

    /// Test-only fixture: the exact mock-parity 3-device/2-candidate seed
    /// data `MockDaemonRepository` also ships, plus a `Connected` initial
    /// link state — every daemon test predating this split was written
    /// against exactly this fixture and asserts on its specific device
    /// ids/candidates/local device name, so tests opt into it explicitly
    /// here rather than a real `flow-daemon` process ever seeding it
    /// implicitly. [`Self::from_storage`] above is what production
    /// actually uses.
    ///
    /// Deliberately plain `pub`, not `#[cfg(test)]`: `daemon/tests/*.rs`
    /// integration tests link this crate as a normal dependency, without
    /// `cfg(test)`, so a `cfg(test)`-gated item would be invisible to
    /// them. The name and doc comment are the guardrail instead — no
    /// production code calls this, and
    /// `production_init_never_seeds_mock_parity_data` (below) fails loudly
    /// if that ever changes.
    #[doc(hidden)]
    pub async fn seeded_for_test(storage: &Storage) -> Self {
        let settings_repo = SettingsRepo::new(storage.clone());
        let device_repo = DeviceRepo::new(storage.clone());

        let settings = settings_repo.load().await;

        let existing = device_repo.list().await;
        let mut devices: HashMap<DeviceId, Device> = if existing.is_empty() {
            let seed = mock_parity_device_records();
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
        restore_local_device_active(&mut devices);

        Self {
            devices,
            link_state: DaemonLinkState::Connected,
            pairing_session: PairingSession::idle(),
            settings,
            permission: PermissionStatus {
                name: "Accessibility access".to_string(),
                granted: false,
            },
            candidates_pool: mock_parity_candidates(),
            discovered_candidates: HashMap::new(),
            incoming_request: None,
        }
    }
}

/// Puts this machine back in [`DeviceState::Active`] after a reload.
///
/// `DeviceRepo` deliberately never persists [`DeviceState`] — a device
/// loaded from disk always comes back `Disconnected` until a live
/// connection re-establishes it, so a stale row can't resurrect a peer as
/// `Active`. That rule is right for *peers* and wrong for this machine:
/// nothing ever re-establishes a connection to the computer the daemon is
/// already running on, so without this every boot after the first left
/// `LOCAL_DEVICE_ID` `Disconnected` too — with no device in any
/// switchable state, `switch_active_device` rejected every target with
/// `device_not_switchable`, the switch key became a permanent no-op, and
/// the Flutter UI's "Controlling" card had nothing to show, since it
/// reads straight off `watch_devices`/`watch_link_state` with no
/// fallback of its own.
///
/// `data-model.md`'s `active` row is the contract this restores: "this
/// machine, 'This device', is `active` by default when nothing else is."
fn restore_local_device_active(devices: &mut HashMap<DeviceId, Device>) {
    if devices.values().any(|d| d.state == DeviceState::Active) {
        return;
    }
    if let Some(local) = devices.get_mut(&DeviceId(LOCAL_DEVICE_ID.to_string())) {
        local.state = DeviceState::Active;
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

/// The device a local hotkey trigger should switch to: the first
/// `Inactive`/`Connected` device in id order starting just after the
/// currently active one, wrapping around — `None` if nothing else is
/// switchable. `Disconnected` devices are skipped, matching
/// `switch_active_device`'s own eligibility rule.
fn next_switchable_device(state: &ServiceState) -> Option<DeviceId> {
    let mut ids: Vec<&DeviceId> = state.devices.keys().collect();
    ids.sort_by(|a, b| a.0.cmp(&b.0));

    let is_switchable = |id: &DeviceId| {
        matches!(
            state.devices[id].state,
            DeviceState::Inactive | DeviceState::Connected
        )
    };

    let active_index = state
        .devices
        .values()
        .find(|device| device.state == DeviceState::Active)
        .and_then(|active| ids.iter().position(|id| **id == active.id));
    let start = active_index.map_or(0, |index| index + 1);

    ids.iter()
        .cycle()
        .skip(start)
        .take(ids.len())
        .find(|id| is_switchable(id))
        .map(|id| (*id).clone())
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
    /// This daemon's own persisted `H1` identity — used to authenticate
    /// and encrypt real pairing handshakes (`NoiseChannel`, `H3`) rather
    /// than trusting a peer's self-reported name outright. See
    /// `request_real_pairing`/`accept_pairing_request`.
    identity: DeviceIdentity,
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
    /// Number of IPC clients (Flutter UIs) currently connected. Zero
    /// means an incoming pairing request has no one to prompt and is
    /// rejected outright — see `accept_pairing_over`.
    connected_clients: Arc<AtomicUsize>,
    /// Broadcasts the single in-flight incoming pairing request (or
    /// `None` when nothing is pending) to subscribed UIs.
    incoming_request_tx: watch::Sender<Option<IncomingPairingRequest>>,
}

impl DaemonService {
    /// Production entry point — `main.rs`'s only constructor call. Real
    /// device/settings state via [`ServiceState::from_storage`]; no
    /// seeded mock data.
    pub async fn new(storage: Storage) -> Self {
        let state = ServiceState::from_storage(&storage).await;
        Self::from_state(storage, state).await
    }

    /// Test-only entry point — the mock-parity fixture every daemon test
    /// predating this split was written against. See
    /// [`ServiceState::seeded_for_test`] for why this is a separate,
    /// explicitly-named constructor rather than something `new` falls
    /// into implicitly.
    #[doc(hidden)]
    pub async fn new_seeded_for_test(storage: Storage) -> Self {
        let state = ServiceState::seeded_for_test(&storage).await;
        Self::from_state(storage, state).await
    }

    /// Shared tail of both constructors above: wraps an already-loaded
    /// [`ServiceState`] in its watch channels.
    async fn from_state(storage: Storage, state: ServiceState) -> Self {
        let identity = DeviceIdentity::load_or_generate(storage.clone()).await;

        let (devices_tx, _) = watch::channel(devices_list(&state));
        let (link_state_tx, _) = watch::channel(state.link_state);
        let (pairing_session_tx, _) = watch::channel(state.pairing_session.clone());
        let (settings_tx, _) = watch::channel(state.settings.clone());
        let (permission_tx, _) = watch::channel(state.permission.clone());
        let (incoming_request_tx, _) = watch::channel(None);

        Self {
            state: Arc::new(RwLock::new(state)),
            storage,
            identity,
            devices_tx,
            link_state_tx,
            pairing_session_tx,
            settings_tx,
            permission_tx,
            pairing_timer: Arc::new(Mutex::new(None)),
            connected_clients: Arc::new(AtomicUsize::new(0)),
            incoming_request_tx,
        }
    }

    /// How many IPC clients are connected right now.
    pub fn connected_client_count(&self) -> usize {
        self.connected_clients.load(Ordering::Relaxed)
    }

    /// Registers one live IPC client for the lifetime of the returned
    /// guard. `ipc::server::handle_connection` holds it for the duration
    /// of a connection; tests hold it to simulate a connected UI.
    pub fn register_ipc_client(&self) -> IpcClientGuard {
        self.connected_clients.fetch_add(1, Ordering::Relaxed);
        IpcClientGuard {
            counter: Arc::clone(&self.connected_clients),
        }
    }

    pub fn watch_devices(&self) -> watch::Receiver<Vec<Device>> {
        self.devices_tx.subscribe()
    }

    pub fn watch_link_state(&self) -> watch::Receiver<DaemonLinkState> {
        self.link_state_tx.subscribe()
    }

    /// Updates the daemon's real link-health state — `main.rs`'s peer
    /// connection lifecycle (`run_peer_pipeline`) is the only real
    /// caller, since that's the one place that knows whether a paired
    /// device is actually streaming input right now. Before any
    /// daemon-to-daemon wiring existed, `link_state` was only ever the
    /// static `Connected` value `ServiceState::load_or_seed` (now
    /// `from_storage`/`seeded_for_test`) set once at startup; this is
    /// what makes it reflect
    /// `docs/contracts/daemon-ipc.md`'s transition table for real
    /// connections instead.
    pub fn set_link_state(&self, state: DaemonLinkState) {
        self.link_state_tx.send_replace(state);
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

    /// Subscribes to the single in-flight incoming pairing request. The
    /// value is `Some` between the daemon surfacing a request and it
    /// being resolved (by `respond_to_pairing_request` or the decision
    /// timeout), `None` otherwise.
    pub fn watch_incoming_request(&self) -> watch::Receiver<Option<IncomingPairingRequest>> {
        self.incoming_request_tx.subscribe()
    }

    /// Makes `device_id` the active device, moving whichever device was
    /// previously active back to inactive. Matches
    /// `MockDaemonRepository.switchActiveDevice`, including its debounce
    /// delay (`SWITCH_DEBOUNCE` — not part of
    /// `sharedContractConstants.mockParityTimings`, but present in the
    /// Dart mock it mirrors; see this module's `buildNote` in
    /// `daemon/todos.json`).
    #[tracing::instrument(skip(self))]
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

    /// Advances to the next switchable device for the local hotkey
    /// trigger path (`daemon/todos.json` F2) — unlike
    /// `switch_active_device`, there's no requester to reject with an
    /// error and no target device id (a physical key press names no
    /// device), so a press with nothing else switchable simply does
    /// nothing observable. Cycles in device-id order starting just after
    /// whichever device is currently active, generalizing vision.md's
    /// two-device Scroll Lock toggle example to any number of devices.
    /// No debounce here — that's F3's job, one layer up in the runner
    /// that actually reads raw (and possibly repeating) key events.
    pub async fn switch_active_device_local(&self) {
        let target_id = {
            let state = self.state.read().await;
            next_switchable_device(&state)
        };
        let Some(target_id) = target_id else {
            return;
        };

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
    }

    /// Removes a paired device. "This device" (`LOCAL_DEVICE_ID`) is
    /// never removable, matching the mock.
    #[tracing::instrument(skip(self))]
    pub async fn remove_device(&self, device_id: &str) -> Result<(), FlowError> {
        if device_id == LOCAL_DEVICE_ID {
            return Err(FlowError::DeviceNotRemovable(DeviceId(
                device_id.to_string(),
            )));
        }

        let target_id = DeviceId(device_id.to_string());
        let (devices, no_paired_devices_left) = {
            let mut state = self.state.write().await;
            if state.devices.remove(&target_id).is_none() {
                return Err(FlowError::DeviceNotFound(target_id));
            }
            let none_left = !state.devices.keys().any(|id| id.0 != LOCAL_DEVICE_ID);
            (devices_list(&state), none_left)
        };

        // Removal is durable: a removed device must not reappear from a
        // stale devices-table row after a restart.
        DeviceRepo::new(self.storage.clone())
            .remove(target_id)
            .await;

        self.devices_tx.send_replace(devices);

        // Removing the last paired device means there is nothing left for
        // the link to legitimately be `Connected` to. `main.rs`'s peer
        // pipeline sets `Connected` and only clears it when its channel
        // actually breaks, which a removal doesn't trigger on its own —
        // so the UI would keep showing "Connected" to a device the user
        // just removed. Reflect reality here instead.
        if no_paired_devices_left
            && !matches!(*self.link_state_tx.borrow(), DaemonLinkState::Disconnected)
        {
            self.set_link_state(DaemonLinkState::Disconnected);
        }

        Ok(())
    }

    /// Begins searching for pairing candidates. Errors if a pairing
    /// session is already active anywhere but idle.
    #[tracing::instrument(skip(self))]
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
    #[tracing::instrument(skip(self))]
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
    /// Requires the session to be in `Found`. A candidate that came from
    /// a live discovery (`note_discovered_peer`) is paired with over a
    /// real `Channel` handshake; a mock `candidates_pool` candidate
    /// keeps the original timer-only mock-parity flow.
    #[tracing::instrument(skip(self))]
    pub async fn pair_with_candidate(&self, candidate_id: &str) -> Result<(), FlowError> {
        let (candidate, address) = {
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
            let address = state
                .discovered_candidates
                .get(candidate_id)
                .map(|(_, address)| address.clone());
            state.pairing_session.stage = PairingStage::Requesting;
            state.pairing_session.target_name = Some(candidate.name.clone());
            (candidate, address)
        };
        self.emit_pairing_session().await;

        let service = self.clone();
        let handle = match address {
            Some(address) => {
                tokio::spawn(
                    async move { service.on_real_pairing_request(candidate, address).await },
                )
            }
            None => tokio::spawn(async move { service.on_pair_request_elapsed(candidate).await }),
        };
        self.set_pairing_timer(handle).await;
        Ok(())
    }

    /// `Searching -> Found`, offering whichever of `candidates_pool`
    /// (the mock-parity fallback) and `discovered_candidates` (real,
    /// live-discovered peers) isn't already a known device name —
    /// matches `MockDaemonRepository.startPairing`'s `_later` callback,
    /// extended with G7's real candidates.
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
                .cloned()
                .chain(state.discovered_candidates.values().map(|(c, _)| c.clone()))
                .filter(|c| !known.contains(c.name.as_str()))
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

    /// Registers a live-discovered peer (`discovery::tcp`/`::bluetooth`)
    /// as a pairing candidate. If a search is already past `Searching`
    /// (i.e. `Found`), folds it into the current session's candidate
    /// list immediately, since `on_search_elapsed` already ran and won't
    /// re-run to pick it up on its own; a peer discovered during
    /// `Searching` or before `start_pairing` is simply cached here for
    /// whichever `on_search_elapsed`/`start_pairing` runs next.
    pub async fn note_discovered_peer(&self, peer: DiscoveredPeer) {
        let candidate_id = format!("live:{}", peer.name);
        let candidate = PairingCandidate {
            id: candidate_id.clone(),
            name: peer.name.clone(),
            os: peer.os,
        };
        let should_emit = {
            let mut state = self.state.write().await;
            state
                .discovered_candidates
                .insert(candidate_id.clone(), (candidate.clone(), peer.address));
            let already_known = state.devices.values().any(|d| d.name == candidate.name);
            let already_offered = state
                .pairing_session
                .candidates
                .iter()
                .any(|c| c.id == candidate.id);
            if !already_known
                && !already_offered
                && state.pairing_session.stage == PairingStage::Found
            {
                state.pairing_session.candidates.push(candidate);
                true
            } else {
                false
            }
        };
        if should_emit {
            self.emit_pairing_session().await;
        }
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

        self.schedule_terminal_to_idle(PairingStage::Paired).await;
    }

    /// `Requesting -> Paired` or `Requesting -> Failed`, driven by a real,
    /// Noise-authenticated handshake over whichever `Channel` medium
    /// `connect_best_available` (G6) negotiates for `address` — the
    /// real-network counterpart to `on_pair_request_elapsed`'s timer-only
    /// mock flow, taken whenever `candidate` came from a live discovery
    /// rather than the mock `candidates_pool`.
    ///
    /// The paired device's stable identity is the peer's `H1` public key
    /// proven by that handshake (`DeviceId` derived from it, per the
    /// review gap on name-based identity — a peer's self-reported
    /// `device_name` is display metadata, never the trust/identity key),
    /// and that same public key is what gets persisted, so `H2`'s trust
    /// gate can recognize this device on a future connection regardless
    /// of which name it presents then.
    async fn on_real_pairing_request(&self, candidate: PairingCandidate, address: ChannelAddress) {
        match self.request_real_pairing(&address).await {
            Ok((PairingDecision::Accept, peer_public_key)) => {
                // `candidate.name`/`.os` (this device's own discovery-time
                // record of the peer) are still the best display metadata
                // available: the pairing handshake itself only carries a
                // decision back to the initiator, not the responder's own
                // name/OS. The device's *identity*, unlike its display
                // name, comes from the handshake's proven public key, not
                // from anything either side merely claims.
                let device_id = device_id_from_public_key(&peer_public_key);
                let new_device = {
                    let mut state = self.state.write().await;
                    if state.pairing_session.stage != PairingStage::Requesting {
                        return;
                    }
                    let device = Device {
                        id: device_id.clone(),
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
                        public_key: Some(peer_public_key),
                        removable: true,
                    })
                    .await;

                self.take_pairing_timer().await;
                self.emit_devices().await;
                self.emit_pairing_session().await;
                self.schedule_terminal_to_idle(PairingStage::Paired).await;
            }
            Ok((PairingDecision::Reject, _peer_public_key)) => {
                self.fail_pairing("the peer rejected the pairing request".to_string())
                    .await;
            }
            Err(err) => {
                self.fail_pairing(format!("pairing failed: {err}")).await;
            }
        }
    }

    /// Negotiates a `Channel` for `address` (G6), authenticates and
    /// encrypts it with a Noise handshake keyed by this daemon's own
    /// `H1` identity (`NoiseChannel::initiate`), then runs the initiator
    /// side of the pairing exchange (`channel::handshake`) over the
    /// resulting encrypted channel — not the raw negotiated one. Written
    /// entirely against the `Channel` trait — no branch on `ChannelKind`
    /// anywhere in this method or in `channel::handshake` — proof that
    /// G1's abstraction holds all the way up to pairing. Returns the
    /// peer's proven public key alongside the decision so the caller
    /// never has to fall back to trusting the peer's self-reported name
    /// as its identity.
    async fn request_real_pairing(
        &self,
        address: &ChannelAddress,
    ) -> Result<(PairingDecision, Vec<u8>), ChannelError> {
        let channel = negotiate::connect_best_available(std::slice::from_ref(address)).await?;
        let mut noise_channel = NoiseChannel::initiate(channel, &self.identity).await?;
        let peer_public_key = noise_channel.peer_identity().to_bytes().to_vec();

        let (local_name, local_os) = self.local_device_identity().await;
        let request = PairingRequest {
            device_name: local_name,
            device_os: local_os,
            // This device's own reachable address isn't tracked yet —
            // that would need a self-advertised listening address,
            // which is out of this task's scope (see this task's
            // buildNote) — left blank rather than fabricated.
            address: String::new(),
        };
        let decision = handshake::request_pairing(&mut noise_channel, request).await?;
        Ok((decision, peer_public_key))
    }

    /// This device's own name/OS, as already recorded for
    /// [`LOCAL_DEVICE_ID`] — used to fill out an outgoing
    /// `PairingRequest`. Falls back to a generic name if the local
    /// device record is ever missing, which shouldn't happen in
    /// practice since `ServiceState::from_storage`/`seeded_for_test`
    /// always seeds it.
    async fn local_device_identity_inner(&self) -> (String, HostOs) {
        let state = self.state.read().await;
        state
            .devices
            .get(&DeviceId(LOCAL_DEVICE_ID.to_string()))
            .map(|device| (device.name.clone(), device.os))
            .unwrap_or_else(|| ("Flow".to_string(), HostOs::Linux))
    }

    /// Accepts an already-negotiated `Channel` from an incoming pairing
    /// attempt (a peer's own `pair_with_candidate`), authenticates and
    /// encrypts it with a Noise handshake keyed by this daemon's own
    /// `H1` identity (`NoiseChannel::accept` — the responder counterpart
    /// to `request_real_pairing`'s `::initiate`), runs the responder side
    /// of the pairing exchange over the resulting encrypted channel, and
    /// — on acceptance — upserts the initiator as a paired device on this
    /// side too, keyed by its proven public key rather than the
    /// self-reported `device_name` in its `PairingRequest` (a claimed
    /// name is display metadata, never a trust identity), so
    /// `docs/product/vision.md` §16's "once accepted, the devices become
    /// trusted" holds symmetrically for both ends of one handshake, not
    /// just the initiator's.
    ///
    /// **Only accepts once the local user has approved the request** —
    /// see [`Self::accept_pairing_over`]. The daemon surfaces the request
    /// to the connected UI over `watch_incoming_request` and blocks this
    /// handshake on [`Self::respond_to_pairing_request`], rejecting on the
    /// user's behalf after [`PAIRING_DECISION_TIMEOUT`] or immediately
    /// when no UI is connected to prompt.
    pub async fn accept_pairing_request(
        &self,
        channel: Box<dyn Channel>,
        peer_addr: Option<SocketAddr>,
    ) -> Result<(), ChannelError> {
        let mut noise_channel = NoiseChannel::accept(channel, &self.identity).await?;
        let peer_public_key = noise_channel.peer_identity().to_bytes().to_vec();
        self.accept_pairing_over(&mut noise_channel, peer_public_key, peer_addr)
            .await
    }

    /// The post-handshake half of [`Self::accept_pairing_request`],
    /// factored out so `main.rs`'s incoming peer-connection dispatcher
    /// (`accept_incoming_peer_channel`, below) can reuse it: that
    /// dispatcher has to run the Noise handshake itself *before* it can
    /// tell a pairing attempt apart from an already-trusted peer's
    /// reconnect (there's no proven identity to branch on until the
    /// handshake produces one), so it can't go through
    /// `accept_pairing_request` without handshaking twice on the same
    /// connection.
    ///
    /// Reads the peer's `PairingRequest`, then gates trust on an explicit
    /// local decision: with no UI connected there is nobody to consent so
    /// the request is rejected outright; otherwise it is published on
    /// `watch_incoming_request` and this call blocks until
    /// [`Self::respond_to_pairing_request`] delivers a decision or
    /// [`PAIRING_DECISION_TIMEOUT`] elapses (⇒ Reject). Only one request
    /// is entertained at a time — a second concurrent one is rejected
    /// while the first is pending. This method is the sole owner of
    /// clearing the pending slot and its watch value.
    async fn accept_pairing_over(
        &self,
        channel: &mut dyn Channel,
        peer_public_key: Vec<u8>,
        peer_addr: Option<SocketAddr>,
    ) -> Result<(), ChannelError> {
        let request = handshake::recv_pairing_request(channel).await?;

        // No UI connected ⇒ nobody can consent. Reject outright, without
        // publishing anything.
        if self.connected_client_count() == 0 {
            tracing::info!("declined an incoming pairing request: no UI connected to prompt");
            return handshake::send_pairing_decision(channel, PairingDecision::Reject).await;
        }

        let info = IncomingPairingRequest {
            request_id: format!("ipr-{:032x}", rand::random::<u128>()),
            device_name: request.device_name.clone(),
            device_os: request.device_os,
            fingerprint: key_fingerprint(&peer_public_key),
            address: peer_addr.map(|a| a.ip().to_string()).unwrap_or_default(),
        };

        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.write().await;
            if state.incoming_request.is_some() {
                tracing::info!(
                    "declined an incoming pairing request: another request is awaiting a decision"
                );
                drop(state);
                return handshake::send_pairing_decision(channel, PairingDecision::Reject).await;
            }
            state.incoming_request = Some(PendingPairingRequest {
                info: info.clone(),
                responder: tx,
            });
        }
        self.incoming_request_tx.send_replace(Some(info));

        let decision = tokio::select! {
            received = rx => received.unwrap_or(PairingDecision::Reject),
            _ = tokio::time::sleep(PAIRING_DECISION_TIMEOUT) => {
                tracing::info!("incoming pairing request timed out with no decision");
                PairingDecision::Reject
            }
        };

        {
            let mut state = self.state.write().await;
            state.incoming_request = None;
        }
        self.incoming_request_tx.send_replace(None);

        handshake::send_pairing_decision(channel, decision).await?;
        if decision != PairingDecision::Accept {
            return Ok(());
        }

        let device_id = device_id_from_public_key(&peer_public_key);
        let device = Device {
            id: device_id.clone(),
            name: request.device_name,
            os: request.device_os,
            state: DeviceState::Inactive,
            last_seen: Utc::now(),
        };
        {
            let mut state = self.state.write().await;
            state.devices.insert(device_id, device.clone());
        }
        DeviceRepo::new(self.storage.clone())
            .upsert(DeviceRecord {
                device,
                public_key: Some(peer_public_key),
                removable: true,
            })
            .await;
        self.emit_devices().await;
        Ok(())
    }

    /// Delivers the local user's Accept/Reject for a pending incoming
    /// pairing request. [`Self::accept_pairing_over`] owns clearing the
    /// pending slot and emitting the `None` watch value, so this only
    /// routes the decision to the blocked handshake.
    ///
    /// `Err(FlowError::PairingRequestNotFound)` when `request_id` matches
    /// nothing currently pending — a stale response, a typo, or a race
    /// with the decision timeout.
    pub async fn respond_to_pairing_request(
        &self,
        request_id: &str,
        decision: PairingDecision,
    ) -> Result<(), FlowError> {
        let responder = {
            let mut state = self.state.write().await;
            match &state.incoming_request {
                Some(pending) if pending.info.request_id == request_id => {
                    state.incoming_request.take().map(|p| p.responder)
                }
                _ => None,
            }
        };
        match responder {
            Some(tx) => {
                // A send failure means the acceptor already timed out and
                // dropped `rx` — harmless, the decision is moot.
                let _ = tx.send(decision);
                Ok(())
            }
            None => Err(FlowError::PairingRequestNotFound),
        }
    }

    /// Accepts one incoming daemon-to-daemon connection on the peer
    /// channel listener (`main.rs`, not yet wired anywhere before this):
    /// runs the Noise handshake first (there's no peer identity to check
    /// against the trust store before it completes), then branches on
    /// whether the resulting proven identity already belongs to a paired
    /// device. An already-trusted peer is handed back to the caller as a
    /// live, authenticated [`Channel`] to run the input-streaming
    /// pipeline over; an untrusted peer is treated as a pairing attempt
    /// and handled in place via [`Self::accept_pairing_over`] — which
    /// blocks on an explicit local decision, so an unattended daemon
    /// rejects it rather than silently trusting whoever asked.
    pub async fn accept_incoming_peer_channel(
        &self,
        channel: Box<dyn Channel>,
        peer_addr: Option<SocketAddr>,
    ) -> Result<IncomingPeerConnection, ChannelError> {
        let noise_channel = NoiseChannel::accept(channel, &self.identity).await?;
        let peer_public_key = noise_channel.peer_identity().to_bytes().to_vec();

        let trust = TrustGate::new(self.storage.clone());
        if trust.is_trusted(&peer_public_key).await {
            let device_id = device_id_from_public_key(&peer_public_key);
            return Ok(IncomingPeerConnection::TrustedPeer(
                Box::new(noise_channel),
                device_id,
                self.connection_precedence(&peer_public_key),
            ));
        }

        let mut noise_channel = noise_channel;
        self.accept_pairing_over(&mut noise_channel, peer_public_key, peer_addr)
            .await?;
        Ok(IncomingPeerConnection::HandledAsPairing)
    }

    /// Which of two simultaneously-established connections to the same
    /// peer both sides should keep.
    ///
    /// Two paired daemons starting together both dial each other at once
    /// (discovery announces immediately on startup), so each ends up
    /// with two connections to the other: one it opened, one it
    /// accepted. Without a rule they agree on, each side independently
    /// keeps "the one I opened" and drops the other's — killing *both*
    /// connections and leaving the pair unable to talk at all.
    ///
    /// The rule: keep the connection opened by whichever side has the
    /// numerically smaller identity public key. Both ends know both keys
    /// once the handshake completes, and the comparison is symmetric, so
    /// they always reach the same verdict — one connection survives,
    /// exactly one is dropped. Keys are unique per device (H1), so this
    /// can't tie.
    fn connection_precedence(&self, peer_public_key: &[u8]) -> ConnectionPrecedence {
        if self.identity.public_key_bytes().as_slice() < peer_public_key {
            // We're the designated dialer, so our *outbound* connection
            // is the one to keep — this inbound one loses.
            ConnectionPrecedence::Redundant
        } else {
            ConnectionPrecedence::Preferred
        }
    }

    /// The outbound counterpart to [`Self::accept_incoming_peer_channel`]:
    /// given a live-discovered peer's address, negotiates a `Channel`
    /// (`G6`) and runs the Noise handshake as initiator to find out who's
    /// actually there, purely by proven identity — never by the
    /// self-reported name a discovery announce carries, per this
    /// codebase's own standing rule that a name is never a safe way to
    /// pick out a specific device. Returns `Some` only when that identity
    /// is already a paired device, i.e. this is a reconnect to maintain,
    /// not a fresh peer to surface through the pairing flow (that path
    /// stays `note_discovered_peer`'s job, unaffected by this method).
    /// Bounded by [`DIAL_TIMEOUT`] end to end: a discovery announce is
    /// unauthenticated, so anything on the network can name an address
    /// that accepts TCP and then simply never completes the Noise
    /// handshake. Without a deadline that stalls this call forever, and
    /// with it a caller that awaits it.
    pub async fn dial_if_trusted(
        &self,
        address: ChannelAddress,
    ) -> Option<(Box<dyn Channel>, DeviceId, ConnectionPrecedence)> {
        // A daemon that has never paired with anything has nothing to
        // reconnect to — skip the whole dial (a TCP connect plus a full
        // Noise handshake, repeated for every discovered peer on every
        // announce tick) rather than run it only to fail the trust check
        // at the end. This is the common case for two fresh daemons on a
        // LAN before the user has paired them through the UI.
        let trust = TrustGate::new(self.storage.clone());
        if !trust.has_any_trusted().await {
            return None;
        }

        let dial = async {
            let channel = negotiate::connect_best_available(std::slice::from_ref(&address))
                .await
                .ok()?;
            NoiseChannel::initiate(channel, &self.identity).await.ok()
        };
        let noise_channel = match tokio::time::timeout(DIAL_TIMEOUT, dial).await {
            Ok(Some(channel)) => channel,
            Ok(None) => return None,
            Err(_) => {
                tracing::debug!("dialing {address:?} timed out before the handshake completed");
                return None;
            }
        };
        let peer_public_key = noise_channel.peer_identity().to_bytes().to_vec();

        if !trust.is_trusted(&peer_public_key).await {
            let mut noise_channel = noise_channel;
            let _ = noise_channel.close().await;
            return None;
        }
        let device_id = device_id_from_public_key(&peer_public_key);
        // Inverted relative to the inbound case: `connection_precedence`
        // answers "should the connection *they* opened win," so for one
        // we opened ourselves the verdict flips.
        let precedence = match self.connection_precedence(&peer_public_key) {
            ConnectionPrecedence::Preferred => ConnectionPrecedence::Redundant,
            ConnectionPrecedence::Redundant => ConnectionPrecedence::Preferred,
        };
        let channel: Box<dyn Channel> = Box::new(noise_channel);
        Some((channel, device_id, precedence))
    }

    /// A stable, per-daemon identifier for `main.rs`'s discovery
    /// announces to tag themselves with, so a broadcast echoing back to
    /// this same host is recognizable as our own rather than treated as
    /// a newly discovered peer (see `discovery::tcp`'s `Announce`).
    ///
    /// This daemon's `H1` identity public key, hex-encoded: unique by
    /// construction and already persisted, so no second identifier has
    /// to be invented or stored. Note this deliberately *publishes* the
    /// public key on the local network — public keys are not secrets,
    /// and the peer listener proves possession of the matching private
    /// key via Noise before anything is trusted, so a copied id buys an
    /// attacker nothing beyond being ignored by the daemon it copied.
    pub fn local_instance_id(&self) -> String {
        hex_encode(&self.identity.public_key_bytes())
    }

    /// This device's own name/OS as recorded for [`LOCAL_DEVICE_ID`] —
    /// exposed for `main.rs`'s discovery-announce loop, which needs to
    /// advertise this daemon's real name/OS rather than a hardcoded
    /// placeholder.
    pub async fn local_device_identity(&self) -> (String, HostOs) {
        self.local_device_identity_inner().await
    }

    /// Moves the pairing session to `Failed` with `message`, unless it
    /// has already moved on (e.g. `cancel_pairing` raced it back to
    /// `Idle`), and schedules the same `Failed -> Idle` timer
    /// `Paired -> Idle` uses.
    async fn fail_pairing(&self, message: String) {
        {
            let mut state = self.state.write().await;
            if state.pairing_session.stage != PairingStage::Requesting {
                return;
            }
            state.pairing_session.stage = PairingStage::Failed;
            state.pairing_session.error = Some(message);
        }
        self.take_pairing_timer().await;
        self.emit_pairing_session().await;
        self.schedule_terminal_to_idle(PairingStage::Failed).await;
    }

    /// Spawns the `expected -> Idle` timer (`Paired -> Idle` or
    /// `Failed -> Idle`) and installs it as the pending pairing timer.
    async fn schedule_terminal_to_idle(&self, expected: PairingStage) {
        let service = self.clone();
        let handle = tokio::spawn(async move { service.on_terminal_elapsed(expected).await });
        self.set_pairing_timer(handle).await;
    }

    /// `Paired -> Idle` or `Failed -> Idle`, automatically, after
    /// `PAIRING_TERMINAL_TO_IDLE` — matches
    /// `MockDaemonRepository.pairWithCandidate`'s innermost `_later` for
    /// the `Paired` case; the `Failed` case is this task's own addition,
    /// since the mock-only flow never reaches `Failed` on its own.
    async fn on_terminal_elapsed(&self, expected: PairingStage) {
        tokio::time::sleep(PAIRING_TERMINAL_TO_IDLE).await;
        {
            let mut state = self.state.write().await;
            if state.pairing_session.stage != expected {
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
    #[tracing::instrument(skip(self))]
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
    #[tracing::instrument(skip(self))]
    pub async fn update_settings(&self, patch: SettingsPatch) -> Result<(), FlowError> {
        self.apply_settings_patch(patch).await;
        Ok(())
    }

    /// Restores [`FlowSettings::defaults`] exactly.
    #[tracing::instrument(skip(self))]
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
    #[tracing::instrument(skip(self))]
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

    /// Reissues a connection attempt after the link has been given up on
    /// — `docs/contracts/daemon-ipc.md`'s `disconnected --(user
    /// retries)--> connecting` and `error --(user retries)--> connecting`
    /// transitions. Only moves the state to `Connecting`, never straight
    /// to `Connected`: an ordinary trusted-peer reconnect is discovered
    /// and dialed automatically the same way the original connection
    /// was (`main.rs`'s discovery loop calls `dial_if_trusted` on every
    /// announce, independent of this command), so this command's job is
    /// only to give the user an honest "trying again" signal rather than
    /// one that claims the link is already back before it is.
    #[tracing::instrument(skip(self))]
    pub async fn retry_connection(&self) -> Result<(), FlowError> {
        let current = *self.link_state_tx.borrow();
        if !matches!(
            current,
            DaemonLinkState::Disconnected | DaemonLinkState::Error
        ) {
            return Err(FlowError::LinkNotRecoverable(current));
        }
        self.link_state_tx.send_replace(DaemonLinkState::Connecting);
        Ok(())
    }
}

/// Decrements the connected-client count when dropped.
pub struct IpcClientGuard {
    counter: Arc<AtomicUsize>,
}

impl Drop for IpcClientGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
    }
}

/// A real paired device's stable identity: derived from its proven `H1`
/// public key, not its self-reported name — multiple machines can
/// legitimately advertise the same display name (`daemon/todos.json`'s
/// review gap #4: "MacBook Pro" / "MacBook Pro" / "MacBook Pro" are not
/// the same trust identity just because they share a name), but two
/// public keys are never accidentally equal.
fn device_id_from_public_key(public_key: &[u8]) -> DeviceId {
    DeviceId(format!("pk:{}", hex_encode(public_key)))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// This daemon's own OS, as compiled — the binary only ever runs on the
/// platform it was built for, so a compile-time constant is exactly as
/// accurate as any runtime check would be, with no extra dependency.
fn current_host_os() -> HostOs {
    match std::env::consts::OS {
        "macos" => HostOs::Macos,
        "windows" => HostOs::Windows,
        _ => HostOs::Linux,
    }
}

/// The OS input-capture permission this daemon starts with, per platform.
///
/// Only macOS gates low-level input capture behind a user-granted
/// permission (Accessibility), and only there can the daemon meaningfully
/// ask for it — so macOS starts `granted: false` and the UI's "Allow"
/// flow is real. Windows (`WH_KEYBOARD_LL`/`SendInput`) and Linux
/// (evdev/uinput, gated by install-time group membership the running
/// process can't change) need nothing the UI could grant at runtime, so
/// they start `granted: true` with an accurate name rather than showing
/// the user a permission prompt that does nothing.
fn default_permission() -> PermissionStatus {
    match current_host_os() {
        HostOs::Macos => PermissionStatus {
            name: "Accessibility access".to_string(),
            granted: false,
        },
        HostOs::Windows => PermissionStatus {
            name: "Input monitoring".to_string(),
            granted: true,
        },
        HostOs::Linux => PermissionStatus {
            name: "Input device access".to_string(),
            granted: true,
        },
    }
}

/// This machine's real hostname, for the local device's display name —
/// falls back to a generic label if the OS ever refuses to report one
/// (rare, and not worth failing daemon startup over a display string).
///
/// `FLOW_DEVICE_NAME` overrides it outright. Its only intended use is
/// running two `flow-daemon` instances on one physical machine for local
/// pairing tests, where both would otherwise report the identical
/// hostname and be impossible to tell apart in the UI.
fn local_hostname() -> String {
    if let Some(name) = std::env::var_os("FLOW_DEVICE_NAME") {
        if let Ok(name) = name.into_string() {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    hostname::get()
        .ok()
        .and_then(|name| name.into_string().ok())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "This device".to_string())
}

/// The real local device record [`ServiceState::from_storage`] seeds on
/// a fresh (empty) database: this machine's own real hostname and OS,
/// active, never removable — the one device every fresh install
/// legitimately has, as opposed to the fake remote devices
/// `mock_parity_device_records` seeds for tests.
fn real_local_device_record() -> DeviceRecord {
    DeviceRecord {
        device: Device {
            id: DeviceId(LOCAL_DEVICE_ID.to_string()),
            name: local_hostname(),
            os: current_host_os(),
            state: DeviceState::Active,
            last_seen: Utc::now(),
        },
        public_key: None,
        removable: false,
    }
}

/// The exact mock-parity 3-device seed data `MockDaemonRepository` also
/// ships — used only by [`ServiceState::seeded_for_test`], never by
/// production's [`ServiceState::from_storage`].
fn mock_parity_device_records() -> Vec<DeviceRecord> {
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

/// The exact mock-parity pairing-candidate seed data
/// `MockDaemonRepository` also ships — used only by
/// [`ServiceState::seeded_for_test`], never by production's
/// [`ServiceState::from_storage`] (whose `candidates_pool` starts empty;
/// real candidates only ever come from `note_discovered_peer`).
fn mock_parity_candidates() -> Vec<PairingCandidate> {
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
        let state = ServiceState::seeded_for_test(&storage).await;

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

        // The real (production) loader — proves the anti-reseed guarantee
        // that actually matters: a non-empty database is never seeded
        // over, mock or real.
        let state = ServiceState::from_storage(&storage).await;

        assert_eq!(state.devices.len(), 1);
        assert!(state
            .devices
            .contains_key(&DeviceId("only-device".to_string())));
    }

    // --- Real (production) fresh-state behavior: the actual subject of
    // this task, "remove mock runtime data from Flow" — a fresh install
    // must show exactly the real local device, no fake remotes, no fake
    // pairing candidates, and no claimed connection until one really
    // exists. Every test above/below this block that uses
    // `DaemonService::new_seeded_for_test`/`ServiceState::seeded_for_test`
    // is deliberately still exercising the mock-parity fixture instead —
    // these are the ones that exercise what a real `flow-daemon` process
    // (`DaemonService::new`/`ServiceState::from_storage`, `main.rs`'s only
    // call site) actually does.

    #[tokio::test]
    async fn fresh_real_state_has_only_the_local_device_no_fake_remotes() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let state = ServiceState::from_storage(&storage).await;

        assert_eq!(
            state.devices.len(),
            1,
            "a fresh real install must have exactly the local device, no \
             seeded fake remotes: {:?}",
            state.devices.values().map(|d| &d.name).collect::<Vec<_>>()
        );
        let local = &state.devices[&DeviceId(LOCAL_DEVICE_ID.to_string())];
        assert_eq!(local.state, DeviceState::Active);
        // The real machine's own identity, not the mock fixture's
        // hardcoded "MacBook"/macOS.
        assert_eq!(local.name, local_hostname());
        assert_eq!(local.os, current_host_os());

        let device_repo = DeviceRepo::new(storage);
        let local_record = device_repo
            .find_by_id(DeviceId(LOCAL_DEVICE_ID.to_string()))
            .await
            .expect("local device persisted");
        assert!(!local_record.removable);
    }

    #[tokio::test]
    async fn fresh_real_state_starts_disconnected_with_no_seeded_candidates() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let state = ServiceState::from_storage(&storage).await;

        assert_eq!(
            state.link_state,
            DaemonLinkState::Disconnected,
            "a fresh daemon has proven no real peer connection yet, so it \
             must not claim Connected"
        );
        assert!(
            state.candidates_pool.is_empty(),
            "production must never seed fake pairing candidates: {:?}",
            state.candidates_pool
        );
        assert_eq!(state.pairing_session, PairingSession::idle());
    }

    #[tokio::test]
    async fn starting_pairing_on_a_real_daemon_finds_no_candidates_without_real_discovery() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;
        let mut sessions = service.watch_pairing_session();
        let _ = sessions.borrow_and_update();

        service.start_pairing().await.expect("start pairing");
        sessions.changed().await.expect("searching update");
        assert_eq!(sessions.borrow_and_update().stage, PairingStage::Searching);

        sessions.changed().await.expect("found update");
        let found = sessions.borrow_and_update().clone();
        assert_eq!(found.stage, PairingStage::Found);
        assert!(
            found.candidates.is_empty(),
            "with no real peer ever discovered, Pair New Device must offer \
             nothing — not the mock's Office Mac Mini/Studio Linux: {:?}",
            found.candidates
        );
    }

    #[tokio::test]
    async fn a_real_discovered_peer_is_the_only_pairing_candidate_offered() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;
        let mut sessions = service.watch_pairing_session();
        let _ = sessions.borrow_and_update();

        service.start_pairing().await.expect("start pairing");
        sessions.changed().await.expect("searching update");

        let peer = DiscoveredPeer {
            name: "Real Linux Box".to_string(),
            os: HostOs::Linux,
            address: ChannelAddress::Tcp("127.0.0.1:9".parse().unwrap()),
        };
        service.note_discovered_peer(peer.clone()).await;

        sessions.changed().await.expect("found update");
        let found = sessions.borrow_and_update().clone();
        assert_eq!(found.stage, PairingStage::Found);
        assert_eq!(
            found.candidates.iter().map(|c| &c.name).collect::<Vec<_>>(),
            vec![&peer.name],
            "only the real discovered peer should be offered, no mock \
             candidates mixed in: {:?}",
            found.candidates
        );
    }

    #[tokio::test]
    async fn a_real_paired_device_survives_a_restart() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage.clone()).await;

        let device_repo = DeviceRepo::new(storage.clone());
        device_repo
            .upsert(DeviceRecord {
                device: Device {
                    id: DeviceId("pk:real-peer".to_string()),
                    name: "Real Windows PC".to_string(),
                    os: HostOs::Windows,
                    state: DeviceState::Connected,
                    last_seen: Utc::now(),
                },
                public_key: Some(vec![7; 32]),
                removable: true,
            })
            .await;

        // Simulate a restart against the same database — the real
        // production path, not the mock fixture.
        drop(service);
        let reloaded = ServiceState::from_storage(&storage).await;

        assert_eq!(
            reloaded.devices.len(),
            2,
            "local device + the real paired one"
        );
        assert!(reloaded
            .devices
            .contains_key(&DeviceId("pk:real-peer".to_string())));
    }

    #[tokio::test]
    async fn removing_a_real_device_persists_across_a_restart() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage.clone()).await;

        let device_repo = DeviceRepo::new(storage.clone());
        device_repo
            .upsert(DeviceRecord {
                device: Device {
                    id: DeviceId("pk:removable-peer".to_string()),
                    name: "Removable Peer".to_string(),
                    os: HostOs::Linux,
                    state: DeviceState::Connected,
                    last_seen: Utc::now(),
                },
                public_key: Some(vec![9; 32]),
                removable: true,
            })
            .await;
        // Reload so the service's in-memory state actually sees the device
        // just inserted directly through the repo above.
        drop(service);
        let service = DaemonService::new(storage.clone()).await;

        service
            .remove_device("pk:removable-peer")
            .await
            .expect("remove the real peer");

        drop(service);
        let reloaded = ServiceState::from_storage(&storage).await;
        assert!(
            !reloaded
                .devices
                .contains_key(&DeviceId("pk:removable-peer".to_string())),
            "a removed device must never reappear on restart, real or not"
        );
    }

    #[tokio::test]
    async fn removing_the_last_paired_device_drops_link_state_to_disconnected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        DeviceRepo::new(storage.clone())
            .upsert(DeviceRecord {
                device: Device {
                    id: DeviceId("pk:only-peer".to_string()),
                    name: "Only Peer".to_string(),
                    os: HostOs::Linux,
                    state: DeviceState::Connected,
                    last_seen: Utc::now(),
                },
                public_key: Some(vec![7; 32]),
                removable: true,
            })
            .await;
        let service = DaemonService::new(storage).await;
        // Simulate the peer pipeline having marked the link healthy.
        service.set_link_state(DaemonLinkState::Connected);

        service
            .remove_device("pk:only-peer")
            .await
            .expect("remove the only paired peer");

        assert_eq!(
            *service.watch_link_state().borrow(),
            DaemonLinkState::Disconnected,
            "removing the last paired device must stop the UI showing a live link to it"
        );
    }

    #[tokio::test]
    async fn fresh_state_permission_matches_this_platform() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;
        let permission = service.watch_permission().borrow().clone();

        // macOS is the only platform whose input capture the UI can
        // actually gate on a user grant; Windows/Linux need nothing the
        // app can do, so they must not start out asking.
        match current_host_os() {
            HostOs::Macos => {
                assert!(!permission.granted);
                assert_eq!(permission.name, "Accessibility access");
            }
            HostOs::Windows | HostOs::Linux => {
                assert!(
                    permission.granted,
                    "non-macOS platforms must not present an unsatisfiable permission prompt"
                );
            }
        }
    }

    /// The guard the task explicitly asked for: this must fail if
    /// production's `DaemonService::new`/`ServiceState::from_storage` are
    /// ever pointed back at the mock-parity fixture
    /// (`seeded_for_test`/`mock_parity_device_records`/
    /// `mock_parity_candidates`). If someone "fixes a bug" by swapping
    /// `from_storage` for `seeded_for_test` inside `DaemonService::new`,
    /// this is what catches it.
    #[tokio::test]
    async fn production_init_never_seeds_mock_parity_data() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        let devices = service.watch_devices().borrow().clone();
        assert_eq!(
            devices.len(),
            1,
            "production seeded more than the real local device: {devices:?}"
        );
        assert!(
            !devices
                .iter()
                .any(|d| d.name == "Work Laptop" || d.name == "Desktop"),
            "production must never seed the mock-parity fake remote devices: {devices:?}"
        );

        assert_eq!(
            *service.watch_link_state().borrow(),
            DaemonLinkState::Disconnected,
            "production must never claim Connected before a real peer link exists"
        );
    }

    #[tokio::test]
    async fn a_subscriber_immediately_sees_the_seeded_state_with_no_prior_emit() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;

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
        let service = DaemonService::new_seeded_for_test(storage).await;

        let a = service.watch_devices();
        let b = service.watch_devices();
        assert_eq!(a.borrow().len(), b.borrow().len());
    }

    #[tokio::test(start_paused = true)]
    async fn switching_to_a_missing_or_active_device_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;

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
        let service = DaemonService::new_seeded_for_test(storage).await;
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
    async fn switch_active_device_local_cycles_through_switchable_devices_and_back() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;
        // Seed: d1 active, d2 inactive, d3 disconnected.

        service.switch_active_device_local().await;
        let devices = service.watch_devices().borrow().clone();
        let by_id =
            |devices: &[Device], id: &str| devices.iter().find(|d| d.id.0 == id).unwrap().state;
        assert_eq!(by_id(&devices, "d1"), DeviceState::Inactive);
        assert_eq!(by_id(&devices, "d2"), DeviceState::Active);
        assert_eq!(by_id(&devices, "d3"), DeviceState::Disconnected);

        // d3 stays disconnected (ineligible), so a second press wraps
        // back to d1 rather than trying d3.
        service.switch_active_device_local().await;
        let devices = service.watch_devices().borrow().clone();
        assert_eq!(by_id(&devices, "d1"), DeviceState::Active);
        assert_eq!(by_id(&devices, "d2"), DeviceState::Inactive);
    }

    #[tokio::test]
    async fn switch_active_device_local_with_nothing_else_switchable_does_nothing() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;
        service.remove_device("d2").await.expect("remove d2");
        // Only d1 (active) and d3 (disconnected) remain — nothing eligible.

        let before = service.watch_devices().borrow().clone();
        service.switch_active_device_local().await;
        let after = service.watch_devices().borrow().clone();
        assert_eq!(before, after);
    }

    /// Regression: `DeviceRepo` never persists `DeviceState`, so every
    /// device — including this machine — used to come back `Disconnected`
    /// on the second boot against the same database. With nothing in a
    /// switchable state, `switch_active_device` rejected every target and
    /// the hotkey became a permanent no-op; the Flutter "Controlling" card
    /// had nothing to show either, since it reads straight off
    /// `watch_devices`.
    #[tokio::test]
    async fn a_second_boot_still_has_a_switchable_device() {
        let storage = Storage::open_in_memory().await.expect("open db");

        // First boot seeds and persists the device list.
        let _ = DaemonService::new_seeded_for_test(storage.clone()).await;

        // Second boot against the same database.
        let service = DaemonService::new_seeded_for_test(storage).await;
        let devices = service.watch_devices().borrow().clone();
        let local = devices
            .iter()
            .find(|d| d.id.0 == LOCAL_DEVICE_ID)
            .expect("the local device survives a reload");
        assert_eq!(local.state, DeviceState::Active);

        service.switch_active_device_local().await;
        let after = service.watch_devices().borrow().clone();
        assert!(
            after.iter().any(|d| d.state == DeviceState::Active),
            "the switch key must still be able to move the active device"
        );
    }

    #[tokio::test]
    async fn removing_the_local_device_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;

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
        let service = DaemonService::new_seeded_for_test(storage).await;

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
        let service = DaemonService::new_seeded_for_test(storage.clone()).await;

        service.remove_device("d3").await.expect("remove d3");
        let devices = service.watch_devices().borrow().clone();
        assert!(!devices.iter().any(|d| d.id.0 == "d3"));

        // Simulate a restart against the same database.
        let reloaded = ServiceState::seeded_for_test(&storage).await;
        assert!(!reloaded.devices.contains_key(&DeviceId("d3".to_string())));
    }

    #[tokio::test]
    async fn start_pairing_while_already_pairing_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;

        service.start_pairing().await.expect("first start_pairing");
        assert_eq!(
            service.start_pairing().await,
            Err(FlowError::PairingInProgress)
        );
    }

    #[tokio::test]
    async fn pair_with_candidate_before_found_or_with_unknown_id_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;

        assert_eq!(
            service.pair_with_candidate("cand-office-mini").await,
            Err(FlowError::PairingNotReady)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn pair_with_candidate_with_an_unknown_id_once_found_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;
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
        let service = DaemonService::new_seeded_for_test(storage).await;

        assert_eq!(
            service.cancel_pairing().await,
            Err(FlowError::PairingNotActive)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn full_pairing_flow_reaches_paired_then_returns_to_idle() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;
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
        let service = DaemonService::new_seeded_for_test(storage).await;
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
        let service = DaemonService::new_seeded_for_test(storage).await;

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
        let service = DaemonService::new_seeded_for_test(storage).await;

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
        let service = DaemonService::new_seeded_for_test(storage).await;

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
        let service = DaemonService::new_seeded_for_test(storage.clone()).await;

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
        let service = DaemonService::new_seeded_for_test(storage).await;

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
        let service = DaemonService::new_seeded_for_test(storage).await;

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
        let service = DaemonService::new_seeded_for_test(storage).await;

        service
            .request_permission()
            .await
            .expect("grant permission");
        assert_eq!(
            service.request_permission().await,
            Err(FlowError::PermissionAlreadyGranted)
        );
    }

    #[tokio::test]
    async fn retry_connection_moves_disconnected_to_connecting() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;
        service.set_link_state(DaemonLinkState::Disconnected);

        service.retry_connection().await.expect("retry accepted");

        assert_eq!(
            *service.watch_link_state().borrow(),
            DaemonLinkState::Connecting
        );
    }

    #[tokio::test]
    async fn retry_connection_moves_error_to_connecting() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;
        service.set_link_state(DaemonLinkState::Error);

        service.retry_connection().await.expect("retry accepted");

        assert_eq!(
            *service.watch_link_state().borrow(),
            DaemonLinkState::Connecting
        );
    }

    #[tokio::test]
    async fn retry_connection_when_already_connected_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;

        assert_eq!(
            service.retry_connection().await,
            Err(FlowError::LinkNotRecoverable(DaemonLinkState::Connected))
        );
        // Rejected, so the state must not have moved.
        assert_eq!(
            *service.watch_link_state().borrow(),
            DaemonLinkState::Connected
        );
    }

    struct IncomingAttempt {
        initiator: JoinHandle<PairingDecision>,
        acceptor: JoinHandle<Result<IncomingPeerConnection, ChannelError>>,
    }

    /// Spawns one incoming pairing attempt against `service`: a loopback
    /// TCP + Noise initiator that sends a `PairingRequest` for
    /// `device_name`, and an acceptor task running
    /// `accept_incoming_peer_channel` — which now blocks on the local
    /// user's decision, so a test must drive `watch_incoming_request()` /
    /// `respond_to_pairing_request()` (or advance past the timeout)
    /// before awaiting either handle.
    async fn spawn_incoming_pairing(service: &DaemonService, device_name: &str) -> IncomingAttempt {
        use crate::channel::tcp::TcpChannel;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");

        let initiator_identity_storage =
            Storage::open_in_memory().await.expect("open initiator db");
        let initiator_identity = DeviceIdentity::load_or_generate(initiator_identity_storage).await;
        let name = device_name.to_string();
        let initiator = tokio::spawn(async move {
            let tcp = TcpChannel::connect(addr).await.expect("connect");
            let mut noise = NoiseChannel::initiate(tcp, &initiator_identity)
                .await
                .expect("initiator handshake");
            handshake::request_pairing(
                &mut noise,
                PairingRequest {
                    device_name: name,
                    device_os: HostOs::Linux,
                    address: String::new(),
                },
            )
            .await
            .expect("decision")
        });

        let (stream, peer_addr) = listener.accept().await.expect("accept");
        let service = service.clone();
        let acceptor = tokio::spawn(async move {
            let channel: Box<dyn Channel> =
                Box::new(TcpChannel::accept(stream).await.expect("accept ws"));
            service
                .accept_incoming_peer_channel(channel, Some(peer_addr))
                .await
        });

        IncomingAttempt {
            initiator,
            acceptor,
        }
    }

    /// Blocks until `requests` yields a published incoming pairing
    /// request and returns it.
    async fn next_published_request(
        requests: &mut watch::Receiver<Option<IncomingPairingRequest>>,
    ) -> IncomingPairingRequest {
        loop {
            if let Some(info) = requests.borrow_and_update().clone() {
                return info;
            }
            requests
                .changed()
                .await
                .expect("incoming request published");
        }
    }

    /// The happy path: the request is published on the watch channel, the
    /// local user Accepts via `respond_to_pairing_request`, the initiator
    /// sees Accept, the peer is persisted keyed by its proven key, and
    /// the pending slot is cleared.
    #[tokio::test]
    async fn an_incoming_request_is_published_and_accepted_by_respond() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;
        let _ui = service.register_ipc_client();

        let mut requests = service.watch_incoming_request();
        let attempt = spawn_incoming_pairing(&service, "Windows Box").await;

        let pending = next_published_request(&mut requests).await;
        assert_eq!(pending.device_name, "Windows Box");
        assert_eq!(pending.device_os, HostOs::Linux);
        assert!(pending.request_id.starts_with("ipr-"));
        assert_eq!(pending.address, "127.0.0.1");
        assert!(!pending.fingerprint.is_empty());

        service
            .respond_to_pairing_request(&pending.request_id, PairingDecision::Accept)
            .await
            .expect("respond accept");

        assert_eq!(
            attempt.initiator.await.expect("initiator join"),
            PairingDecision::Accept
        );
        let outcome = attempt
            .acceptor
            .await
            .expect("acceptor join")
            .expect("accept ok");
        assert!(matches!(outcome, IncomingPeerConnection::HandledAsPairing));

        assert!(
            service.watch_incoming_request().borrow().is_none(),
            "the pending slot must be cleared once resolved"
        );
        let devices = service.watch_devices().borrow().clone();
        assert!(
            devices.iter().any(|d| d.name == "Windows Box"),
            "an accepted peer must be persisted: {devices:?}"
        );
    }

    /// A Reject decision reaches the initiator and leaves nothing behind.
    #[tokio::test]
    async fn respond_reject_persists_nothing() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;
        let _ui = service.register_ipc_client();

        let mut requests = service.watch_incoming_request();
        let attempt = spawn_incoming_pairing(&service, "Rejected Box").await;
        let pending = next_published_request(&mut requests).await;

        service
            .respond_to_pairing_request(&pending.request_id, PairingDecision::Reject)
            .await
            .expect("respond reject");

        assert_eq!(
            attempt.initiator.await.expect("initiator join"),
            PairingDecision::Reject
        );
        attempt
            .acceptor
            .await
            .expect("acceptor join")
            .expect("acceptor ok");

        assert!(service.watch_incoming_request().borrow().is_none());
        let devices = service.watch_devices().borrow().clone();
        assert!(
            !devices.iter().any(|d| d.name == "Rejected Box"),
            "a rejected peer must not be persisted: {devices:?}"
        );
    }

    /// No decision within `PAIRING_DECISION_TIMEOUT` ⇒ the daemon rejects
    /// on the user's behalf; the initiator sees Reject and nothing is
    /// persisted.
    #[tokio::test(start_paused = true)]
    async fn no_answer_auto_rejects_after_timeout() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;
        let _ui = service.register_ipc_client();

        let mut requests = service.watch_incoming_request();
        let attempt = spawn_incoming_pairing(&service, "Silent Box").await;
        let pending = next_published_request(&mut requests).await;
        assert!(pending.request_id.starts_with("ipr-"));

        tokio::time::advance(PAIRING_DECISION_TIMEOUT + Duration::from_millis(1)).await;

        assert_eq!(
            attempt.initiator.await.expect("initiator join"),
            PairingDecision::Reject
        );
        attempt
            .acceptor
            .await
            .expect("acceptor join")
            .expect("acceptor ok");

        assert!(service.watch_incoming_request().borrow().is_none());
        let devices = service.watch_devices().borrow().clone();
        assert!(!devices.iter().any(|d| d.name == "Silent Box"));
    }

    /// With no IPC client connected there is nobody to prompt, so the
    /// request is rejected outright — never published, never persisted.
    #[tokio::test]
    async fn no_connected_ui_rejects_immediately() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;
        // Deliberately no `register_ipc_client()`.

        let attempt = spawn_incoming_pairing(&service, "Uninvited Box").await;

        assert_eq!(
            attempt.initiator.await.expect("initiator join"),
            PairingDecision::Reject
        );
        attempt
            .acceptor
            .await
            .expect("acceptor join")
            .expect("acceptor ok");

        assert!(
            service.watch_incoming_request().borrow().is_none(),
            "nothing may be published when no UI can consent"
        );
        let devices = service.watch_devices().borrow().clone();
        assert!(!devices.iter().any(|d| d.name == "Uninvited Box"));
    }

    /// Only one request at a time: a second attempt arriving while one is
    /// pending is rejected immediately without displacing the first.
    #[tokio::test]
    async fn a_second_request_while_one_is_pending_is_rejected() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;
        let _ui = service.register_ipc_client();

        let mut requests = service.watch_incoming_request();
        let first = spawn_incoming_pairing(&service, "First Box").await;
        let pending = next_published_request(&mut requests).await;
        assert_eq!(pending.device_name, "First Box");

        let second = spawn_incoming_pairing(&service, "Second Box").await;
        assert_eq!(
            second.initiator.await.expect("second initiator join"),
            PairingDecision::Reject
        );
        second
            .acceptor
            .await
            .expect("second acceptor join")
            .expect("second acceptor ok");

        let still = service
            .watch_incoming_request()
            .borrow()
            .clone()
            .expect("first request still pending");
        assert_eq!(still.request_id, pending.request_id);

        service
            .respond_to_pairing_request(&pending.request_id, PairingDecision::Accept)
            .await
            .expect("respond accept");
        assert_eq!(
            first.initiator.await.expect("first initiator join"),
            PairingDecision::Accept
        );
        first
            .acceptor
            .await
            .expect("first acceptor join")
            .expect("first acceptor ok");

        let devices = service.watch_devices().borrow().clone();
        assert!(devices.iter().any(|d| d.name == "First Box"));
        assert!(
            !devices.iter().any(|d| d.name == "Second Box"),
            "the rejected second peer must not be persisted: {devices:?}"
        );
        assert!(service.watch_incoming_request().borrow().is_none());
    }

    /// `respond_to_pairing_request` with an id that matches nothing
    /// pending is a `PairingRequestNotFound` error.
    #[tokio::test]
    async fn respond_with_unknown_id_errs() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;

        let err = service
            .respond_to_pairing_request("ipr-nope", PairingDecision::Accept)
            .await
            .unwrap_err();
        assert_eq!(err, FlowError::PairingRequestNotFound);
    }

    #[tokio::test]
    async fn accept_incoming_peer_channel_hands_back_a_live_channel_for_an_already_trusted_peer() {
        use crate::channel::tcp::TcpChannel;
        use tokio::net::TcpListener;

        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage.clone()).await;

        let peer_identity_storage = Storage::open_in_memory().await.expect("open peer db");
        let peer_identity = DeviceIdentity::load_or_generate(peer_identity_storage).await;
        let peer_public_key = peer_identity.public_key_bytes().to_vec();
        DeviceRepo::new(storage.clone())
            .upsert(DeviceRecord {
                device: Device {
                    id: DeviceId("peer".to_string()),
                    name: "Trusted Peer".to_string(),
                    os: HostOs::Linux,
                    state: DeviceState::Inactive,
                    last_seen: Utc::now(),
                },
                public_key: Some(peer_public_key.clone()),
                removable: true,
            })
            .await;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let initiator = tokio::spawn(async move {
            let tcp = TcpChannel::connect(addr).await.expect("connect");
            NoiseChannel::initiate(tcp, &peer_identity)
                .await
                .expect("initiator handshake")
        });

        let (stream, _peer) = listener.accept().await.expect("accept");
        let channel: Box<dyn Channel> =
            Box::new(TcpChannel::accept(stream).await.expect("accept ws"));
        let outcome = service
            .accept_incoming_peer_channel(channel, None)
            .await
            .expect("accept incoming");
        match outcome {
            IncomingPeerConnection::TrustedPeer(_, device_id, _) => {
                assert_eq!(device_id, device_id_from_public_key(&peer_public_key));
            }
            IncomingPeerConnection::HandledAsPairing => {
                panic!("expected a trusted peer, not a pairing attempt")
            }
        }
        initiator.await.expect("initiator task");
    }

    #[tokio::test]
    async fn dial_if_trusted_is_none_when_the_peer_at_that_address_is_unpaired() {
        use crate::channel::tcp::TcpChannel;
        use tokio::net::TcpListener;

        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage).await;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let responder_identity_storage = Storage::open_in_memory().await.expect("open db");
        let responder_identity = DeviceIdentity::load_or_generate(responder_identity_storage).await;
        tokio::spawn(async move {
            let (stream, _peer) = listener.accept().await.expect("accept");
            let tcp = TcpChannel::accept(stream).await.expect("accept ws");
            let _ = NoiseChannel::accept(tcp, &responder_identity).await;
        });

        let result = service.dial_if_trusted(ChannelAddress::Tcp(addr)).await;
        assert!(result.is_none());
    }
}
