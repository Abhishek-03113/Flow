//! Versioned schema (`daemon/todos.json` `persistenceModel.tables`):
//! `settings` (single row), `devices` (paired devices + trust public key),
//! `connection_history` (append-only event log), `identity` (this
//! daemon's own keypair, one row). Applied via [`rusqlite_migration`] so
//! opening a database always leaves it at the latest schema version,
//! never requiring a manual migrate step.

use rusqlite_migration::{Migrations, M};

/// The single initial migration. Split into further `M::up(...)` entries
/// (appended, never edited in place) the day the schema needs to change.
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(
        "
        CREATE TABLE devices (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            os          TEXT NOT NULL,
            last_seen   TEXT NOT NULL,
            public_key  BLOB,
            removable   INTEGER NOT NULL DEFAULT 1
        );

        CREATE TABLE connection_history (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id    TEXT NOT NULL,
            event_type   TEXT NOT NULL,
            occurred_at  TEXT NOT NULL,
            detail       TEXT
        );
        CREATE INDEX idx_connection_history_device_time
            ON connection_history (device_id, occurred_at);

        CREATE TABLE settings (
            id                           INTEGER PRIMARY KEY CHECK (id = 1),
            launch_at_login              INTEGER NOT NULL,
            show_tray_icon               INTEGER NOT NULL,
            auto_reconnect               INTEGER NOT NULL,
            auto_connect_paired_devices  INTEGER NOT NULL,
            share_keyboard               INTEGER NOT NULL,
            share_mouse                  INTEGER NOT NULL,
            debug_logging                INTEGER NOT NULL,
            pointer_sensitivity          TEXT NOT NULL,
            switch_key_label             TEXT NOT NULL,
            switch_key_tokens            TEXT NOT NULL
        );

        CREATE TABLE identity (
            id           INTEGER PRIMARY KEY CHECK (id = 1),
            public_key   BLOB NOT NULL,
            private_key  BLOB NOT NULL
        );
        ",
    )])
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;

    fn table_names(conn: &Connection) -> Vec<String> {
        // AUTOINCREMENT on connection_history implicitly creates
        // sqlite_sequence — not one of our tables, excluded here.
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name != 'sqlite_sequence'
                 ORDER BY name",
            )
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    #[test]
    fn fresh_database_gets_all_four_tables() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrations().to_latest(&mut conn).unwrap();

        assert_eq!(
            table_names(&conn),
            vec!["connection_history", "devices", "identity", "settings"]
        );
    }

    #[test]
    fn reapplying_migrations_on_an_already_migrated_db_is_a_no_op() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrations().to_latest(&mut conn).unwrap();
        migrations().to_latest(&mut conn).unwrap();

        assert_eq!(
            table_names(&conn),
            vec!["connection_history", "devices", "identity", "settings"]
        );
    }
}
