//! The pairing trust gate (`daemon/todos.json` H2): whether an incoming
//! connection's claimed identity belongs to an already-paired device,
//! per `P4`'s device repository — the single source of trust
//! (`docs/product/vision.md` §16: "Once accepted, the devices become
//! trusted"). Medium-agnostic by construction: this module never names
//! a concrete `Channel` type, only a public key, so it runs the same
//! way regardless of whether the connection arrived over `TcpChannel`
//! or `BluetoothChannel`.
//!
//! **Scope note.** Consulting this gate at the moment a *real* incoming
//! connection is accepted needs cryptographic proof that the peer
//! actually holds the private key matching whatever public key it
//! claims — an unauthenticated TCP/Bluetooth connection can claim any
//! public key it likes, so the gate alone can't yet be wired into a
//! live accept path without that proof. That proof is `H3`'s Noise
//! handshake (which authenticates the connection itself); wiring this
//! gate into a live `Channel`-accept path is `H4`'s job — its own
//! `dependsOn` lists both this task and `H3`, not this one alone. This
//! task's own scope, per its acceptance criteria, is the gate function
//! itself, tested directly against `P4`'s repository.

use crate::storage::device_repo::DeviceRepo;
use crate::storage::Storage;

/// Consults `P4`'s device repository to decide whether a peer claiming
/// `public_key` is an already-trusted (paired) device.
#[derive(Clone)]
pub struct TrustGate {
    device_repo: DeviceRepo,
}

impl TrustGate {
    pub fn new(storage: Storage) -> Self {
        Self {
            device_repo: DeviceRepo::new(storage),
        }
    }

    /// Whether `public_key` belongs to a device already paired with this
    /// daemon — true once, and only once, `DeviceRepo::upsert` has
    /// stored it, which is what `G7`'s pairing handshake does on a
    /// successful `PairingDecision::Accept`.
    pub async fn is_trusted(&self, public_key: &[u8]) -> bool {
        self.device_repo.is_trusted(public_key.to_vec()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::device_repo::DeviceRecord;
    use flow_core::device::{Device, DeviceId, DeviceState, HostOs};

    fn a_paired_device_record(public_key: Vec<u8>) -> DeviceRecord {
        DeviceRecord {
            device: Device {
                id: DeviceId("d2".to_string()),
                name: "Paired Device".to_string(),
                os: HostOs::Linux,
                state: DeviceState::Inactive,
                last_seen: chrono::Utc::now(),
            },
            public_key: Some(public_key),
            removable: true,
        }
    }

    #[tokio::test]
    async fn a_public_key_upserted_via_p4_is_reported_trusted() {
        let storage = Storage::open_in_memory().await.expect("open db");
        DeviceRepo::new(storage.clone())
            .upsert(a_paired_device_record(vec![1, 2, 3]))
            .await;

        let gate = TrustGate::new(storage);
        assert!(gate.is_trusted(&[1, 2, 3]).await);
    }

    #[tokio::test]
    async fn an_unknown_public_key_is_not_trusted() {
        let storage = Storage::open_in_memory().await.expect("open db");
        DeviceRepo::new(storage.clone())
            .upsert(a_paired_device_record(vec![1, 2, 3]))
            .await;

        let gate = TrustGate::new(storage);
        assert!(!gate.is_trusted(&[9, 9, 9]).await);
    }

    #[tokio::test]
    async fn a_never_paired_database_trusts_nothing() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let gate = TrustGate::new(storage);
        assert!(!gate.is_trusted(&[1, 2, 3]).await);
    }
}
