//! `FlowError`: the single place every contract error code is defined
//! (`docs/contracts/daemon-ipc.md`, one variant per precondition).
//!
//! Command handlers in `flow-daemon` return `Result<_, FlowError>` and
//! never construct an ad-hoc error string — the same discipline
//! `docs/contracts/README.md` ground rule 1 asks of the Dart side
//! (`DaemonCommandException`).

use thiserror::Error;

use crate::device::DeviceId;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FlowError {
    #[error("device not found: {0:?}")]
    DeviceNotFound(DeviceId),

    #[error("device not switchable: {0:?}")]
    DeviceNotSwitchable(DeviceId),

    #[error("device not removable: {0:?}")]
    DeviceNotRemovable(DeviceId),

    #[error("pairing already in progress")]
    PairingInProgress,

    #[error("no pairing session is active")]
    PairingNotActive,

    #[error("pairing session is not ready to pair yet")]
    PairingNotReady,

    #[error("pairing candidate not found: {0}")]
    CandidateNotFound(String),

    #[error("switch key binding must have at least one key")]
    InvalidSwitchKey,

    #[error("permission already granted")]
    PermissionAlreadyGranted,
}

impl FlowError {
    /// The exact snake_case wire string for this error's code, per
    /// `docs/contracts/daemon-ipc.md`'s error code table.
    pub fn code(&self) -> &'static str {
        match self {
            FlowError::DeviceNotFound(_) => "device_not_found",
            FlowError::DeviceNotSwitchable(_) => "device_not_switchable",
            FlowError::DeviceNotRemovable(_) => "device_not_removable",
            FlowError::PairingInProgress => "pairing_in_progress",
            FlowError::PairingNotActive => "pairing_not_active",
            FlowError::PairingNotReady => "pairing_not_ready",
            FlowError::CandidateNotFound(_) => "candidate_not_found",
            FlowError::InvalidSwitchKey => "invalid_switch_key",
            FlowError::PermissionAlreadyGranted => "permission_already_granted",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_reports_its_contract_code() {
        let id = DeviceId("d1".to_string());
        assert_eq!(
            FlowError::DeviceNotFound(id.clone()).code(),
            "device_not_found"
        );
        assert_eq!(
            FlowError::DeviceNotSwitchable(id.clone()).code(),
            "device_not_switchable"
        );
        assert_eq!(
            FlowError::DeviceNotRemovable(id).code(),
            "device_not_removable"
        );
        assert_eq!(FlowError::PairingInProgress.code(), "pairing_in_progress");
        assert_eq!(FlowError::PairingNotActive.code(), "pairing_not_active");
        assert_eq!(FlowError::PairingNotReady.code(), "pairing_not_ready");
        assert_eq!(
            FlowError::CandidateNotFound("cand-1".to_string()).code(),
            "candidate_not_found"
        );
        assert_eq!(FlowError::InvalidSwitchKey.code(), "invalid_switch_key");
        assert_eq!(
            FlowError::PermissionAlreadyGranted.code(),
            "permission_already_granted"
        );
    }
}
