//! The in-memory service state a `DaemonService` (track B2) wraps in
//! watch channels. `load_or_seed` is the load-or-bootstrap step: a fresh
//! (empty) database looks identical to `MockDaemonRepository`'s seed data
//! (`daemon/todos.json` `sharedContractConstants.mockParitySeedData`) —
//! after that first run, whatever was actually persisted comes back
//! instead.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use flow_core::channel::{Channel, ChannelAddress, ChannelError};
use flow_core::device::{Device, DeviceId, DeviceState, HostOs};
use flow_core::error::FlowError;
use flow_core::link::DaemonLinkState;
use flow_core::pairing::{
    PairingCandidate, PairingDecision, PairingRequest, PairingSession, PairingStage,
};
use flow_core::permission::PermissionStatus;
use flow_core::settings::{FlowSettings, SettingsPatch};
use flow_core::switch_key::SwitchKeyBinding;
use tokio::sync::{watch, Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio::time::Duration;

use crate::channel::noise::NoiseChannel;
use crate::channel::{handshake, negotiate};
use crate::discovery::DiscoveredPeer;
use crate::identity::DeviceIdentity;
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
    TrustedPeer(Box<dyn Channel>, DeviceId),
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
            discovered_candidates: HashMap::new(),
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
}

impl DaemonService {
    pub async fn new(storage: Storage) -> Self {
        let state = ServiceState::load_or_seed(&storage).await;
        let identity = DeviceIdentity::load_or_generate(storage.clone()).await;

        let (devices_tx, _) = watch::channel(devices_list(&state));
        let (link_state_tx, _) = watch::channel(state.link_state);
        let (pairing_session_tx, _) = watch::channel(state.pairing_session.clone());
        let (settings_tx, _) = watch::channel(state.settings.clone());
        let (permission_tx, _) = watch::channel(state.permission.clone());

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
    /// static `Connected` value `ServiceState::load_or_seed` sets once
    /// at startup; this is what makes it reflect
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
    /// practice since `load_or_seed` always seeds it.
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
    /// Always accepts for now: the Flutter-facing contract has no
    /// incoming-pairing-request command/UI yet
    /// (`docs/contracts/daemon-ipc.md`'s `PairingSession` only models the
    /// *initiating* side's view) — a real accept/reject prompt on this
    /// side is a natural follow-up requiring a new contract command,
    /// out of this task's scope; flagged honestly here rather than
    /// silently assumed. See this task's `buildNote`.
    pub async fn accept_pairing_request(
        &self,
        channel: Box<dyn Channel>,
    ) -> Result<(), ChannelError> {
        let mut noise_channel = NoiseChannel::accept(channel, &self.identity).await?;
        let peer_public_key = noise_channel.peer_identity().to_bytes().to_vec();
        self.accept_pairing_over(&mut noise_channel, peer_public_key)
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
    async fn accept_pairing_over(
        &self,
        channel: &mut dyn Channel,
        peer_public_key: Vec<u8>,
    ) -> Result<(), ChannelError> {
        let (request, decision) =
            handshake::respond_to_pairing(channel, |_request| PairingDecision::Accept).await?;
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

    /// Accepts one incoming daemon-to-daemon connection on the peer
    /// channel listener (`main.rs`, not yet wired anywhere before this):
    /// runs the Noise handshake first (there's no peer identity to check
    /// against the trust store before it completes), then branches on
    /// whether the resulting proven identity already belongs to a paired
    /// device. An already-trusted peer is handed back to the caller as a
    /// live, authenticated [`Channel`] to run the input-streaming
    /// pipeline over; an untrusted peer is treated as a pairing attempt
    /// and handled in place via [`Self::accept_pairing_over`] — the same
    /// "always accept" responder behavior `accept_pairing_request` uses.
    pub async fn accept_incoming_peer_channel(
        &self,
        channel: Box<dyn Channel>,
    ) -> Result<IncomingPeerConnection, ChannelError> {
        let noise_channel = NoiseChannel::accept(channel, &self.identity).await?;
        let peer_public_key = noise_channel.peer_identity().to_bytes().to_vec();

        let trust = TrustGate::new(self.storage.clone());
        if trust.is_trusted(&peer_public_key).await {
            let device_id = device_id_from_public_key(&peer_public_key);
            return Ok(IncomingPeerConnection::TrustedPeer(
                Box::new(noise_channel),
                device_id,
            ));
        }

        let mut noise_channel = noise_channel;
        self.accept_pairing_over(&mut noise_channel, peer_public_key)
            .await?;
        Ok(IncomingPeerConnection::HandledAsPairing)
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
    pub async fn dial_if_trusted(
        &self,
        address: ChannelAddress,
    ) -> Option<(Box<dyn Channel>, DeviceId)> {
        let channel = negotiate::connect_best_available(std::slice::from_ref(&address))
            .await
            .ok()?;
        let noise_channel = NoiseChannel::initiate(channel, &self.identity).await.ok()?;
        let peer_public_key = noise_channel.peer_identity().to_bytes().to_vec();

        let trust = TrustGate::new(self.storage.clone());
        if !trust.is_trusted(&peer_public_key).await {
            let mut noise_channel = noise_channel;
            let _ = noise_channel.close().await;
            return None;
        }
        let device_id = device_id_from_public_key(&peer_public_key);
        let channel: Box<dyn Channel> = Box::new(noise_channel);
        Some((channel, device_id))
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
    async fn switch_active_device_local_cycles_through_switchable_devices_and_back() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;
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
        let service = DaemonService::new(storage).await;
        service.remove_device("d2").await.expect("remove d2");
        // Only d1 (active) and d3 (disconnected) remain — nothing eligible.

        let before = service.watch_devices().borrow().clone();
        service.switch_active_device_local().await;
        let after = service.watch_devices().borrow().clone();
        assert_eq!(before, after);
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

    #[tokio::test]
    async fn accept_incoming_peer_channel_treats_an_untrusted_initiator_as_a_pairing_attempt() {
        use crate::channel::tcp::TcpChannel;
        use tokio::net::TcpListener;

        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage).await;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");

        let initiator_identity_storage =
            Storage::open_in_memory().await.expect("open initiator db");
        let initiator_identity = DeviceIdentity::load_or_generate(initiator_identity_storage).await;
        let initiator = tokio::spawn(async move {
            let tcp = TcpChannel::connect(addr).await.expect("connect");
            let mut noise = NoiseChannel::initiate(tcp, &initiator_identity)
                .await
                .expect("initiator handshake");
            handshake::request_pairing(
                &mut noise,
                PairingRequest {
                    device_name: "New Laptop".to_string(),
                    device_os: HostOs::Linux,
                    address: String::new(),
                },
            )
            .await
            .expect("decision")
        });

        let (stream, _peer) = listener.accept().await.expect("accept");
        let channel: Box<dyn Channel> =
            Box::new(TcpChannel::accept(stream).await.expect("accept ws"));
        let outcome = service
            .accept_incoming_peer_channel(channel)
            .await
            .expect("accept incoming");
        assert!(matches!(outcome, IncomingPeerConnection::HandledAsPairing));

        let decision = initiator.await.expect("initiator task");
        assert_eq!(decision, PairingDecision::Accept);

        let devices = service.watch_devices().borrow().clone();
        assert!(devices.iter().any(|d| d.name == "New Laptop"));
    }

    #[tokio::test]
    async fn accept_incoming_peer_channel_hands_back_a_live_channel_for_an_already_trusted_peer() {
        use crate::channel::tcp::TcpChannel;
        use tokio::net::TcpListener;

        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new(storage.clone()).await;

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
            .accept_incoming_peer_channel(channel)
            .await
            .expect("accept incoming");
        match outcome {
            IncomingPeerConnection::TrustedPeer(_, device_id) => {
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
        let service = DaemonService::new(storage).await;

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
