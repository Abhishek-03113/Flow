//! The in-memory service state a `DaemonService` (track B2) wraps in
//! watch channels. `load_or_seed` is the load-or-bootstrap step: a fresh
//! (empty) database looks identical to `MockDaemonRepository`'s seed data
//! (`daemon/todos.json` `sharedContractConstants.mockParitySeedData`) —
//! after that first run, whatever was actually persisted comes back
//! instead.

use std::collections::HashMap;

use chrono::{Duration as ChronoDuration, Utc};
use flow_core::device::{Device, DeviceId, DeviceState, HostOs};
use flow_core::link::DaemonLinkState;
use flow_core::pairing::{PairingCandidate, PairingSession};
use flow_core::permission::PermissionStatus;
use flow_core::settings::FlowSettings;

use crate::storage::device_repo::{DeviceRecord, DeviceRepo};
use crate::storage::settings_repo::SettingsRepo;
use crate::storage::Storage;

/// "This device" — the machine `flow-daemon` itself is running on. Never
/// removable, never offered as a pairing candidate; matches
/// `MockDaemonRepository._localDeviceId`.
pub const LOCAL_DEVICE_ID: &str = "d1";

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
}
