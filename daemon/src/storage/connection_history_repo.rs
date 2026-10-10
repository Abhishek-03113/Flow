//! Append-only connection history log (`docs/archive/tasks/daemon-todos.json` task P5).
//! Rows are written by [`super::history_logger`], which observes
//! `DaemonService`'s watch-channel event bus rather than requiring every
//! command handler to remember to log a transition itself.

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::Row;

use super::{parse_rfc3339, Storage, StorageError};

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionHistoryEntry {
    pub id: i64,
    pub device_id: String,
    pub event_type: String,
    pub occurred_at: DateTime<Utc>,
    pub detail: Option<String>,
}

#[derive(Clone)]
pub struct ConnectionHistoryRepo {
    storage: Storage,
}

impl ConnectionHistoryRepo {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    /// Appends one history row, timestamped now.
    pub async fn append(
        &self,
        device_id: &str,
        event_type: &str,
        detail: Option<&str>,
    ) -> Result<(), StorageError> {
        let device_id = device_id.to_string();
        let event_type = event_type.to_string();
        let detail = detail.map(|d| d.to_string());
        let occurred_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);

        self.storage
            .with_connection(move |conn| {
                conn.execute(
                    "INSERT INTO connection_history (device_id, event_type, occurred_at, detail)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![device_id, event_type, occurred_at, detail],
                )?;
                Ok(())
            })
            .await
    }

    /// The `limit` most recent rows for `device_id`, newest first.
    pub async fn recent(
        &self,
        device_id: &str,
        limit: i64,
    ) -> Result<Vec<ConnectionHistoryEntry>, StorageError> {
        let device_id = device_id.to_string();
        self.storage
            .with_connection(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, device_id, event_type, occurred_at, detail
                         FROM connection_history
                         WHERE device_id = ?1
                         ORDER BY occurred_at DESC, id DESC
                         LIMIT ?2",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![device_id, limit], row_to_entry)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }
}

fn row_to_entry(row: &Row) -> rusqlite::Result<ConnectionHistoryEntry> {
    let occurred_at: String = row.get(3)?;
    Ok(ConnectionHistoryEntry {
        id: row.get(0)?,
        device_id: row.get(1)?,
        event_type: row.get(2)?,
        occurred_at: parse_rfc3339(&occurred_at),
        detail: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn append_then_recent_round_trips_in_newest_first_order() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let repo = ConnectionHistoryRepo::new(storage);

        repo.append("d2", "device_activated", None)
            .await
            .expect("append");
        repo.append("d2", "device_removed", Some("manual"))
            .await
            .expect("append");
        repo.append("d3", "device_activated", None)
            .await
            .expect("append");

        let d2_history = repo.recent("d2", 10).await.expect("recent");
        assert_eq!(d2_history.len(), 2);
        assert_eq!(d2_history[0].event_type, "device_removed");
        assert_eq!(d2_history[0].detail.as_deref(), Some("manual"));
        assert_eq!(d2_history[1].event_type, "device_activated");
    }

    #[tokio::test]
    async fn recent_respects_the_limit() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let repo = ConnectionHistoryRepo::new(storage);

        for _ in 0..5 {
            repo.append("d2", "device_activated", None)
                .await
                .expect("append");
        }

        assert_eq!(repo.recent("d2", 2).await.expect("recent").len(), 2);
    }
}
