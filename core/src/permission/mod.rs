//! OS input-capture permission state (`data-model.md` "PermissionStatus").
//!
//! `name` is daemon-supplied (it already knows the platform it's running
//! on) rather than derived client-side, so the UI never hardcodes per-OS
//! permission copy.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionStatus {
    pub name: String,
    pub granted: bool,
}
