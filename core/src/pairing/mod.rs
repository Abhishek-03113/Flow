//! Pairing request/response types and the `PairingSession` state machine
//! (vision.md §16, Pairing; `docs/contracts/daemon-ipc.md` "Pairing").
//!
//! Secure device identity and key exchange are explicitly future work in
//! the vision doc, so they aren't modeled here yet — `PairingRequest`/
//! `PairingDecision` are only the accept/reject shape needed for
//! local-network pairing, wrapped by `channel::PairingWireMessage`
//! (track G1) for the actual network handshake (track G7).

use serde::{Deserialize, Serialize};

use crate::device::HostOs;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PairingRequest {
    pub device_name: String,
    pub device_os: HostOs,
    pub address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingDecision {
    Accept,
    Reject,
}

/// Stage of the pairing state machine (`docs/contracts/daemon-ipc.md`
/// "Pairing (PairingSession.stage)"):
///
/// `idle --start_pairing--> searching --(candidate found)--> found;
/// found --pair_with_candidate--> requesting --(peer accepts)--> paired
/// --(auto, ~1.6s)--> idle; requesting --(peer rejects/times out)-->
/// failed --(auto, ~1.6s)--> idle; any non-idle state --cancel_pairing-->
/// idle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingStage {
    Idle,
    Searching,
    Found,
    Requesting,
    Paired,
    Failed,
}

/// A discoverable device offered as a pairing target once
/// [`PairingStage::Found`] is reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PairingCandidate {
    pub id: String,
    pub name: String,
    pub os: HostOs,
}

/// An incoming pairing request awaiting the local user's decision,
/// surfaced to the UI as `incoming_pairing_request_changed`
/// (`docs/contracts/daemon-ipc.md`). All fields except `request_id` and
/// `fingerprint` are self-reported by the peer and are display-only;
/// `fingerprint` is a short hash of the peer's *proven* identity key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct IncomingPairingRequest {
    /// Opaque, daemon-generated; echoed back in `respond_to_pairing_request`.
    pub request_id: String,
    pub device_name: String,
    pub device_os: HostOs,
    /// e.g. `"3f2a 91c4 8d10 6b57"` — first 8 bytes of SHA-256 of the
    /// peer's proven ed25519 public key.
    pub fingerprint: String,
    /// Peer source IP, display only; empty when unknown (e.g. Bluetooth).
    pub address: String,
}

/// Current state of the pairing flow, mirroring `data-model.md`'s
/// `PairingSession` class field-for-field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PairingSession {
    pub stage: PairingStage,
    /// Populated once `stage >= Found`.
    pub candidates: Vec<PairingCandidate>,
    /// Set once `stage >= Requesting`.
    pub target_name: Option<String>,
    /// Set only when `stage == Failed`.
    pub error: Option<String>,
}

impl PairingSession {
    /// The idle default: no candidates, no target, no error.
    pub fn idle() -> Self {
        Self {
            stage: PairingStage::Idle,
            candidates: Vec::new(),
            target_name: None,
            error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_has_no_candidates_target_or_error() {
        let session = PairingSession::idle();
        assert_eq!(session.stage, PairingStage::Idle);
        assert!(session.candidates.is_empty());
        assert_eq!(session.target_name, None);
        assert_eq!(session.error, None);
    }

    #[test]
    fn incoming_pairing_request_serializes_snake_case() {
        let req = IncomingPairingRequest {
            request_id: "ipr-abc".to_string(),
            device_name: "Abhishek's Windows".to_string(),
            device_os: HostOs::Windows,
            fingerprint: "3f2a 91c4 8d10 6b57".to_string(),
            address: "192.168.0.103".to_string(),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["request_id"], "ipr-abc");
        assert_eq!(json["device_os"], "windows");
        assert_eq!(json["fingerprint"], "3f2a 91c4 8d10 6b57");
        let round: IncomingPairingRequest = serde_json::from_value(json).unwrap();
        assert_eq!(round, req);
    }
}
