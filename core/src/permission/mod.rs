//! OS input-capture permission state (`data-model.md` "PermissionStatus").
//!
//! `name` is daemon-supplied (it already knows the platform it's running
//! on) rather than derived client-side, so the UI never hardcodes per-OS
//! permission copy.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PermissionStatus {
    pub name: String,
    pub granted: bool,
}
