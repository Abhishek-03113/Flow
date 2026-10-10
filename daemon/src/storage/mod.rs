//! SQLite-backed persistence (`docs/contracts/` companion: this module is
//! the one place `rusqlite` is imported outside its own tests — settings,
//! paired devices/trust, this daemon's identity, and connection history
//! all live here instead of being derived every run or kept only in
//! memory).
//!
//! `rusqlite` is synchronous; every query runs inside
//! `tokio::task::spawn_blocking` against a single connection shared behind
//! `Arc<tokio::sync::Mutex<_>>`, per `docs/archive/tasks/daemon-todos.json`'s
//! `persistenceModel.concurrencyModel`.

pub mod connection_history_repo;
pub mod device_repo;
pub mod history_logger;
pub mod identity_repo;
pub mod layout_repo;
mod schema;
pub mod settings_repo;

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use tokio::sync::Mutex;

/// Everything that can go wrong talking to the database: opening or
/// migrating it, running a query (full disk, locked file, ...), or the
/// blocking task that runs the query failing.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("storage task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("stored value could not be (de)serialized: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<StorageError> for flow_core::error::FlowError {
    fn from(err: StorageError) -> Self {
        flow_core::error::FlowError::Internal(format!("storage: {err}"))
    }
}

/// A handle to the daemon's single SQLite connection. Cheap to clone —
/// clones share the same underlying connection.
#[derive(Clone)]
pub struct Storage {
    conn: Arc<Mutex<Connection>>,
}

impl Storage {
    /// Opens (creating if needed) the database file at `path`, applying
    /// any pending migrations.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        let conn =
            tokio::task::spawn_blocking(move || Self::open_and_configure(Connection::open(path)?))
                .await??;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Opens a private in-memory database, migrated to the latest schema.
    /// Tests use this exclusively so no persistence test ever touches the
    /// real filesystem.
    pub async fn open_in_memory() -> Result<Self, StorageError> {
        let conn =
            tokio::task::spawn_blocking(|| Self::open_and_configure(Connection::open_in_memory()?))
                .await??;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn open_and_configure(mut conn: Connection) -> Result<Connection, StorageError> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::migrations().to_latest(&mut conn)?;
        Ok(conn)
    }

    /// Runs `f` against the shared connection on a blocking-pool thread,
    /// serialized through the connection's mutex. This is the single entry
    /// point every repository (`SettingsRepo`, `DeviceRepo`, ...) builds
    /// its queries on top of.
    pub async fn with_connection<F, T>(&self, f: F) -> Result<T, StorageError>
    where
        F: FnOnce(&mut Connection) -> Result<T, StorageError> + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let mut guard = conn.blocking_lock();
            f(&mut guard)
        })
        .await?
    }
}

/// Parses a stored RFC 3339 timestamp column. Shared by every repo that
/// stores a `chrono::DateTime<Utc>` as `TEXT`, so the parsing/panic
/// message stays in one place instead of being copy-pasted per repo.
pub(crate) fn parse_rfc3339(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .unwrap_or_else(|e| panic!("stored timestamp {s:?} is not RFC 3339: {e}"))
        .with_timezone(&Utc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_in_memory_never_touches_disk_and_is_usable() {
        let storage = Storage::open_in_memory().await.expect("open in-memory db");
        let journal_mode: String = storage
            .with_connection(|conn| {
                Ok(conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?)
            })
            .await
            .expect("read journal_mode");
        // SQLite silently keeps in-memory DBs in "memory" mode regardless
        // of the WAL pragma — this just proves the pragma call didn't error.
        assert_eq!(journal_mode, "memory");
    }

    #[tokio::test]
    async fn concurrent_writers_do_not_deadlock_or_panic() {
        let storage = Storage::open_in_memory().await.expect("open in-memory db");
        storage
            .with_connection(|conn| {
                conn.execute("CREATE TABLE t (id INTEGER PRIMARY KEY)", [])?;
                Ok(())
            })
            .await
            .expect("create test table");

        let mut handles = Vec::new();
        for i in 0..16 {
            let storage = storage.clone();
            handles.push(tokio::spawn(async move {
                storage
                    .with_connection(move |conn| {
                        conn.execute("INSERT INTO t (id) VALUES (?1)", [i])?;
                        Ok(())
                    })
                    .await
                    .expect("insert row");
            }));
        }
        for handle in handles {
            handle.await.expect("writer task panicked");
        }

        let count: i64 = storage
            .with_connection(|conn| {
                Ok(conn.query_row("SELECT COUNT(*) FROM t", [], |row| row.get(0))?)
            })
            .await
            .expect("count rows");
        assert_eq!(count, 16);
    }
}
