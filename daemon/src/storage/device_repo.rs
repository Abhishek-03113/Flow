//! Paired-device storage and trust lookups (`docs/archive/tasks/daemon-todos.json` task P4).
//!
//! A paired device's stored public key *is* its trust record — there is
//! no separate trust file. [`DeviceState`] is never persisted: a device
//! loaded from disk always starts [`DeviceState::Disconnected`] until a
//! live connection re-establishes it, never resurrected as `Active` from
//! a stale row.

use chrono::SecondsFormat;
use flow_core::device::{Device, DeviceId, DeviceState, HostOs};
use rusqlite::{OptionalExtension, Row};

use chrono::{DateTime, Utc};

use super::{parse_rfc3339, Storage, StorageError};

/// A device row as stored on disk: the contract [`Device`] plus the two
/// storage-only concerns (trust key, removability) that aren't part of
/// the UI-facing wire contract.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceRecord {
    pub device: Device,
    pub public_key: Option<Vec<u8>>,
    pub removable: bool,
}

#[derive(Clone)]
pub struct DeviceRepo {
    storage: Storage,
}

impl DeviceRepo {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    pub async fn list(&self) -> Result<Vec<DeviceRecord>, StorageError> {
        self.storage
            .with_connection(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, name, os, last_seen, public_key, removable FROM devices
                         ORDER BY id",
                )?;
                let rows = stmt
                    .query_map([], row_to_record)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    pub async fn find_by_id(&self, id: DeviceId) -> Result<Option<DeviceRecord>, StorageError> {
        self.storage
            .with_connection(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT id, name, os, last_seen, public_key, removable FROM devices
                         WHERE id = ?1",
                        [&id.0],
                        row_to_record,
                    )
                    .optional()?)
            })
            .await
    }

    pub async fn upsert(&self, record: DeviceRecord) -> Result<(), StorageError> {
        self.storage
            .with_connection(move |conn| {
                conn.execute(
                    "INSERT INTO devices (id, name, os, last_seen, public_key, removable)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT (id) DO UPDATE SET
                         name = excluded.name,
                         os = excluded.os,
                         last_seen = excluded.last_seen,
                         public_key = excluded.public_key,
                         removable = excluded.removable",
                    rusqlite::params![
                        record.device.id.0,
                        record.device.name,
                        host_os_to_str(record.device.os),
                        record
                            .device
                            .last_seen
                            .to_rfc3339_opts(SecondsFormat::Secs, true),
                        record.public_key,
                        record.removable,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    /// Persists only a device's `last_seen`. Unlike [`Self::upsert`] this
    /// leaves the trust key and every other column alone, and a missing
    /// row (device removed meanwhile) is a no-op.
    pub async fn set_last_seen(
        &self,
        id: DeviceId,
        last_seen: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        self.storage
            .with_connection(move |conn| {
                conn.execute(
                    "UPDATE devices SET last_seen = ?1 WHERE id = ?2",
                    rusqlite::params![last_seen.to_rfc3339_opts(SecondsFormat::Secs, true), id.0],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn remove(&self, id: DeviceId) -> Result<(), StorageError> {
        self.storage
            .with_connection(move |conn| {
                conn.execute("DELETE FROM devices WHERE id = ?1", [&id.0])?;
                Ok(())
            })
            .await
    }

    /// Whether any stored device has a trust key at all — i.e. this
    /// daemon has completed pairing with at least one peer. Used to skip
    /// the discovery-driven reconnect dial (a full TCP + Noise handshake)
    /// entirely on a daemon that has never paired with anything, where it
    /// could only ever fail.
    pub async fn has_any_trusted(&self) -> Result<bool, StorageError> {
        self.storage
            .with_connection(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT 1 FROM devices WHERE public_key IS NOT NULL LIMIT 1",
                        [],
                        |_| Ok(()),
                    )
                    .optional()?
                    .is_some())
            })
            .await
    }

    /// Whether `public_key` matches a stored device's trust key.
    pub async fn is_trusted(&self, public_key: Vec<u8>) -> Result<bool, StorageError> {
        self.storage
            .with_connection(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT 1 FROM devices WHERE public_key = ?1",
                        [public_key],
                        |_| Ok(()),
                    )
                    .optional()?
                    .is_some())
            })
            .await
    }
}

fn row_to_record(row: &Row) -> rusqlite::Result<DeviceRecord> {
    let id: String = row.get(0)?;
    let os: String = row.get(2)?;
    let last_seen: String = row.get(3)?;

    Ok(DeviceRecord {
        device: Device {
            id: DeviceId(id),
            name: row.get(1)?,
            os: host_os_from_str(&os),
            // Never persisted — always Disconnected until a live
            // connection re-establishes the real state.
            state: DeviceState::Disconnected,
            last_seen: parse_rfc3339(&last_seen),
            role: None,
            screen: None,
            link_error: None,
        },
        public_key: row.get(4)?,
        removable: row.get(5)?,
    })
}

fn host_os_to_str(os: HostOs) -> &'static str {
    match os {
        HostOs::Macos => "macos",
        HostOs::Windows => "windows",
        HostOs::Linux => "linux",
    }
}

fn host_os_from_str(s: &str) -> HostOs {
    match s {
        "windows" => HostOs::Windows,
        "linux" => HostOs::Linux,
        _ => HostOs::Macos,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::*;
    use crate::storage::Storage;

    fn sample(id: &str, public_key: Option<Vec<u8>>) -> DeviceRecord {
        DeviceRecord {
            device: Device {
                id: DeviceId(id.to_string()),
                name: "Work Laptop".to_string(),
                os: HostOs::Windows,
                state: DeviceState::Active,
                last_seen: DateTime::parse_from_rfc3339("2026-08-25T06:58:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                role: None,
                screen: None,
                link_error: None,
            },
            public_key,
            removable: true,
        }
    }

    #[tokio::test]
    async fn upsert_then_reload_round_trips_every_field_except_state() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let repo = DeviceRepo::new(storage);

        let record = sample("d2", Some(vec![1, 2, 3]));
        repo.upsert(record.clone()).await.expect("upsert");

        let reloaded = repo
            .find_by_id(DeviceId("d2".to_string()))
            .await
            .expect("find")
            .expect("device present after upsert");

        assert_eq!(reloaded.device.id, record.device.id);
        assert_eq!(reloaded.device.name, record.device.name);
        assert_eq!(reloaded.device.os, record.device.os);
        assert_eq!(reloaded.device.last_seen, record.device.last_seen);
        assert_eq!(reloaded.public_key, record.public_key);
        assert_eq!(reloaded.removable, record.removable);

        // state is never persisted: an Active row on disk still loads as
        // Disconnected, not resurrected from the stale write.
        assert_eq!(reloaded.device.state, DeviceState::Disconnected);
    }

    #[tokio::test]
    async fn is_trusted_matches_only_a_stored_public_key() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let repo = DeviceRepo::new(storage);

        repo.upsert(sample("d2", Some(vec![9, 9, 9])))
            .await
            .expect("upsert");

        assert!(repo.is_trusted(vec![9, 9, 9]).await.expect("query"));
        assert!(!repo.is_trusted(vec![1, 1, 1]).await.expect("query"));
    }

    #[tokio::test]
    async fn list_and_remove() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let repo = DeviceRepo::new(storage);

        repo.upsert(sample("d2", None)).await.expect("upsert");
        repo.upsert(sample("d3", None)).await.expect("upsert");
        assert_eq!(repo.list().await.expect("list").len(), 2);

        repo.remove(DeviceId("d2".to_string()))
            .await
            .expect("remove");
        let remaining = repo.list().await.expect("list");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].device.id, DeviceId("d3".to_string()));
    }

    #[tokio::test]
    async fn set_last_seen_persists_time_and_keeps_trust_key() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let repo = DeviceRepo::new(storage);
        repo.upsert(sample("d2", Some(vec![4, 5, 6])))
            .await
            .expect("upsert");

        let heard = DateTime::parse_from_rfc3339("2026-09-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        repo.set_last_seen(DeviceId("d2".to_string()), heard)
            .await
            .expect("set last seen");
        // A device that no longer exists is a harmless no-op.
        repo.set_last_seen(DeviceId("gone".to_string()), heard)
            .await
            .expect("no-op");

        let reloaded = repo
            .find_by_id(DeviceId("d2".to_string()))
            .await
            .expect("find")
            .expect("present");
        assert_eq!(reloaded.device.last_seen, heard);
        assert_eq!(reloaded.public_key, Some(vec![4, 5, 6]));
    }
}
