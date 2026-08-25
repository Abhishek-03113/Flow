//! This daemon's own device identity (`daemon/todos.json` H1): a real,
//! mathematically valid ed25519 keypair, generated once and persisted
//! across restarts via track P6's `IdentityRepo`.
//!
//! `storage::identity_repo`'s own contract, by its own doc comment, is
//! deliberately narrow — "the same 64 bytes come back every time," not
//! cryptographic validity, since that repo predates this task and has
//! no `ed25519-dalek` dependency of its own. This module is what turns
//! the persisted bytes into an actual keypair: it treats the repo's
//! persisted `private_key` column as an ed25519 signing-key seed
//! (`ed25519-dalek`'s `SigningKey` is exactly 32 bytes, matching what
//! the repo already stores) and derives the public key from it, rather
//! than trusting the repo's own `public_key` column — which was filled
//! with independently-random bytes that have no cryptographic
//! relationship to the private key at all.

use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};

use crate::storage::identity_repo::IdentityRepo;
use crate::storage::Storage;

/// This daemon's own persisted device identity — an ed25519 keypair.
/// `H2`'s trust gate compares a peer's [`Self::public_key_bytes`]
/// against `P4`'s device repository; `H3`'s `NoiseChannel` uses the
/// full keypair for its handshake.
#[derive(Clone)]
pub struct DeviceIdentity {
    signing_key: SigningKey,
}

impl DeviceIdentity {
    /// Loads this daemon's persisted identity, generating one (via
    /// `P6`'s `IdentityRepo`) on first run. Every later call against the
    /// same `storage` returns the identical keypair.
    pub async fn load_or_generate(storage: Storage) -> Self {
        let bytes = IdentityRepo::new(storage).load_or_generate().await;
        let seed: [u8; 32] = bytes
            .private_key
            .try_into()
            .expect("identity_repo always persists a 32-byte private key");
        Self {
            signing_key: SigningKey::from_bytes(&seed),
        }
    }

    /// This identity's public key.
    pub fn public_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// This identity's public key as raw bytes — the form `P4`'s device
    /// repository (`public_key: Option<Vec<u8>>`) and the wire protocol
    /// actually store/carry.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.public_key().to_bytes()
    }

    /// Signs `message` with this identity's private key.
    pub fn sign(&self, message: &[u8]) -> ed25519_dalek::Signature {
        self.signing_key.sign(message)
    }
}

/// Verifies that `signature` over `message` was produced by the holder
/// of `public_key` — a free function (rather than a method on
/// `DeviceIdentity`, which only ever represents *this* daemon's own
/// identity) since verification is always checking a *peer's* claimed
/// identity.
pub fn verify(
    public_key: &VerifyingKey,
    message: &[u8],
    signature: &ed25519_dalek::Signature,
) -> bool {
    public_key.verify(message, signature).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    #[tokio::test]
    async fn first_run_generates_a_real_verifiable_ed25519_keypair() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let identity = DeviceIdentity::load_or_generate(storage).await;

        let message = b"flow device identity";
        let signature = identity.sign(message);
        assert!(verify(&identity.public_key(), message, &signature));
    }

    #[tokio::test]
    async fn a_second_load_on_the_same_storage_returns_the_identical_identity() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let first = DeviceIdentity::load_or_generate(storage.clone()).await;
        let second = DeviceIdentity::load_or_generate(storage).await;
        assert_eq!(first.public_key_bytes(), second.public_key_bytes());
    }

    #[tokio::test]
    async fn two_separate_databases_generate_different_identities() {
        let storage_a = Storage::open_in_memory().await.expect("open db a");
        let storage_b = Storage::open_in_memory().await.expect("open db b");

        let identity_a = DeviceIdentity::load_or_generate(storage_a).await;
        let identity_b = DeviceIdentity::load_or_generate(storage_b).await;

        assert_ne!(identity_a.public_key_bytes(), identity_b.public_key_bytes());
    }

    #[tokio::test]
    async fn a_signature_does_not_verify_against_a_different_identitys_public_key() {
        let storage_a = Storage::open_in_memory().await.expect("open db a");
        let storage_b = Storage::open_in_memory().await.expect("open db b");

        let identity_a = DeviceIdentity::load_or_generate(storage_a).await;
        let identity_b = DeviceIdentity::load_or_generate(storage_b).await;

        let message = b"flow device identity";
        let signature = identity_a.sign(message);
        assert!(!verify(&identity_b.public_key(), message, &signature));
    }
}
