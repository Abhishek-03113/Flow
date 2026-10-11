//! `FlowError`: the single place every contract error code is defined
//! (`docs/contracts/daemon-ipc.md`, one variant per precondition).
//!
//! Command handlers in `flow-daemon` return `Result<_, FlowError>` and
//! never construct an ad-hoc error string — the same discipline
//! `docs/contracts/README.md` ground rule 1 asks of the UI side
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

    #[error("a shortcut with a typing key needs Ctrl, Alt or Meta")]
    SwitchKeyNeedsModifier,

    #[error("the return-home shortcut is the same as the switch shortcut")]
    SwitchKeyConflict,

    #[error("permission already granted")]
    PermissionAlreadyGranted,

    #[error("link state is {0:?}, not disconnected or error — nothing to retry")]
    LinkNotRecoverable(crate::link::DaemonLinkState),

    #[error("no pairing request is awaiting a decision for that id")]
    PairingRequestNotFound,

    #[error("only a controller can switch screens")]
    NotController,

    #[error("only a controlled device can release control")]
    NotControlled,

    #[error("no controller is linked to this device")]
    NoController,

    #[error("invalid remap rule: {0}")]
    InvalidRemapRule(String),

    #[error("two remap rules for the same key and computer: {0}")]
    DuplicateRemapRule(String),

    #[error("invalid screen layout: {0}")]
    InvalidLayout(String),

    #[error("no permission named {0:?}")]
    PermissionNotFound(String),

    #[error("link state is {0:?}, not reconnecting — nothing to cancel")]
    NotReconnecting(crate::link::DaemonLinkState),

    #[error("internal error: {0}")]
    Internal(String),
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
            FlowError::SwitchKeyNeedsModifier => "switch_key_needs_modifier",
            FlowError::SwitchKeyConflict => "switch_key_conflict",
            FlowError::PermissionAlreadyGranted => "permission_already_granted",
            FlowError::LinkNotRecoverable(_) => "link_not_recoverable",
            FlowError::PairingRequestNotFound => "pairing_request_not_found",
            FlowError::NotController => "not_controller",
            FlowError::NotControlled => "not_controlled",
            FlowError::NoController => "no_controller",
            FlowError::InvalidRemapRule(_) => "invalid_remap_rule",
            FlowError::DuplicateRemapRule(_) => "duplicate_remap_rule",
            FlowError::InvalidLayout(_) => "invalid_layout",
            FlowError::PermissionNotFound(_) => "permission_not_found",
            FlowError::NotReconnecting(_) => "not_reconnecting",
            FlowError::Internal(_) => "internal_error",
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
            FlowError::SwitchKeyNeedsModifier.code(),
            "switch_key_needs_modifier"
        );
        assert_eq!(FlowError::SwitchKeyConflict.code(), "switch_key_conflict");
        assert_eq!(
            FlowError::DuplicateRemapRule("x".to_string()).code(),
            "duplicate_remap_rule"
        );
        assert_eq!(
            FlowError::PermissionAlreadyGranted.code(),
            "permission_already_granted"
        );
        assert_eq!(
            FlowError::LinkNotRecoverable(crate::link::DaemonLinkState::Connected).code(),
            "link_not_recoverable"
        );
        assert_eq!(
            FlowError::PairingRequestNotFound.code(),
            "pairing_request_not_found"
        );
        assert_eq!(FlowError::NotController.code(), "not_controller");
        assert_eq!(FlowError::NotControlled.code(), "not_controlled");
        assert_eq!(FlowError::NoController.code(), "no_controller");
        assert_eq!(
            FlowError::InvalidRemapRule("x".to_string()).code(),
            "invalid_remap_rule"
        );
        assert_eq!(
            FlowError::Internal("x".to_string()).code(),
            "internal_error"
        );
    }
}
