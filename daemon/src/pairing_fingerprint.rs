//! A short, human-comparable fingerprint of a peer's proven identity
//! key, shown in the incoming-pairing prompt so the user has something
//! tied to the *authenticated* key rather than only the peer's
//! self-reported name.

use sha2::{Digest, Sha256};

/// First 8 bytes of SHA-256(`public_key`), as four space-separated
/// 4-hex-digit groups, e.g. `"3f2a 91c4 8d10 6b57"`.
pub fn key_fingerprint(public_key: &[u8]) -> String {
    let digest = Sha256::digest(public_key);
    format!(
        "{:02x}{:02x} {:02x}{:02x} {:02x}{:02x} {:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_stable_and_shaped_for_a_fixed_key() {
        let key = [0u8; 32];
        let fp = key_fingerprint(&key);
        // 4 groups of 4 lowercase hex digits, single-space separated.
        assert_eq!(fp.len(), 19);
        assert_eq!(fp.split(' ').count(), 4);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit() || c == ' '));
        // Deterministic: SHA-256 of 32 zero bytes starts 66687aad...
        assert_eq!(fp, "6668 7aad f862 bd77");
    }

    #[test]
    fn different_keys_give_different_fingerprints() {
        assert_ne!(key_fingerprint(&[0u8; 32]), key_fingerprint(&[1u8; 32]));
    }
}
