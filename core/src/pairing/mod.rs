//! Pairing request/response types (vision.md §16, Pairing).
//!
//! Secure device identity and key exchange are explicitly future work in
//! the vision doc, so they aren't modeled here yet — only the accept/reject
//! shape needed for local-network pairing.

#[derive(Debug, Clone)]
pub struct PairingRequest {
    pub device_name: String,
    pub address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingDecision {
    Accept,
    Reject,
}
