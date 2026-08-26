//! Library surface for `flow-daemon`'s internal modules.
//!
//! `main.rs` builds on this crate rather than declaring these modules
//! itself, so integration tests (`daemon/tests/*.rs`) — which link
//! against the crate, not the binary — can reach `service`/`storage` the
//! same way `main.rs` does.

pub mod channel;
pub mod discovery;
pub mod hotkey;
pub mod identity;
pub mod ipc;
pub mod logging;
pub mod pipeline;
pub mod service;
pub mod storage;
pub mod trust;

/// Lowercase hex for a byte slice — the one rendering of raw bytes as
/// text this crate needs (the IPC auth token, a peer's public key in a
/// `DeviceId`). A four-line helper rather than a `hex` dependency, but
/// one four-line helper: `ipc::auth` and `service` each carried their
/// own byte-identical copy before.
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

#[cfg(test)]
mod hex_encode_tests {
    use super::hex_encode;

    #[test]
    fn encodes_each_byte_as_two_lowercase_hex_digits() {
        assert_eq!(hex_encode(&[]), "");
        assert_eq!(hex_encode(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
    }
}
