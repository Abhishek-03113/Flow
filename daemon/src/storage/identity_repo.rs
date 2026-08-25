//! This daemon's own persisted identity key material
//! (`daemon/todos.json` task P6). Generates and persists a fresh keypair
//! into the `identity` table on first run; every later run loads the
//! same one back, so the daemon's identity survives restarts instead of
//! being re-rolled every process start.
//!
//! `rand` (the only crate imported here, per the project's
//! wrap-third-party-dependencies rule) is used only to fill the raw key
//! bytes. Interpreting those bytes as an actual ed25519 keypair — the
//! cryptographic half of "identity" — is track H1's job
//! (`ed25519-dalek`, layered on top of this repo); this module's
//! contract is purely "the same 64 bytes come back every time", not
//! cryptographic validity.

use rand::Rng;
use rusqlite::OptionalExtension;

use super::Storage;

/// Raw persisted key material: 32 bytes each, matching an ed25519
/// keypair's byte layout so track H1 can adopt these bytes directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityKeyBytes {
    pub public_key: Vec<u8>,
    pub private_key: Vec<u8>,
}

#[derive(Clone)]
pub struct IdentityRepo {
    storage: Storage,
}

impl IdentityRepo {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    /// Loads the persisted identity, generating and persisting a fresh
    /// one on an empty `identity` table (first run).
    pub async fn load_or_generate(&self) -> IdentityKeyBytes {
        self.storage
            .with_connection(|conn| {
                let existing = conn
                    .query_row(
                        "SELECT public_key, private_key FROM identity WHERE id = 1",
                        [],
                        |row| {
                            Ok(IdentityKeyBytes {
                                public_key: row.get(0)?,
                                private_key: row.get(1)?,
                            })
                        },
                    )
                    .optional()
                    .expect("query identity row");

                match existing {
                    Some(keys) => keys,
                    None => {
                        let keys = generate();
                        conn.execute(
                            "INSERT INTO identity (id, public_key, private_key)
                             VALUES (1, ?1, ?2)",
                            rusqlite::params![keys.public_key, keys.private_key],
                        )
                        .expect("insert identity row");
                        keys
                    }
                }
            })
            .await
    }
}

fn generate() -> IdentityKeyBytes {
    let mut public_key = vec![0u8; 32];
    let mut private_key = vec![0u8; 32];
    rand::rng().fill_bytes(&mut public_key);
    rand::rng().fill_bytes(&mut private_key);
    IdentityKeyBytes {
        public_key,
        private_key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    #[tokio::test]
    async fn first_call_generates_second_call_returns_the_same_keys() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let repo = IdentityRepo::new(storage);

        let first = repo.load_or_generate().await;
        assert_eq!(first.public_key.len(), 32);
        assert_eq!(first.private_key.len(), 32);

        let second = repo.load_or_generate().await;
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn a_fresh_storage_handle_on_the_same_db_loads_the_identical_keys() {
        // "same DB file" simulated here by sharing the same in-memory
        // Storage handle rather than reopening a path — open_in_memory()
        // gives each call its own private database, so a fresh handle
        // would not see the first one's data. Persistence across a real
        // restart is exercised by settings_repo/device_repo's equivalent
        // tests against the same underlying mechanism.
        let storage = Storage::open_in_memory().await.expect("open db");
        let repo_a = IdentityRepo::new(storage.clone());
        let repo_b = IdentityRepo::new(storage);

        let generated = repo_a.load_or_generate().await;
        let loaded = repo_b.load_or_generate().await;
        assert_eq!(generated, loaded);
    }
}
