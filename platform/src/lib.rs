//! OS-specific input capture/injection adapters (vision.md §6-§7).
//!
//! Exactly one of these modules compiles for a given build target. The
//! daemon depends only on the traits in `flow_core::input`, never on a
//! concrete adapter type, so this crate is the only place that needs to
//! know which operating system it's running on.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::{LinuxInputCapture, LinuxInputInjector};
#[cfg(target_os = "macos")]
pub use macos::{MacosInputCapture, MacosInputInjector};
#[cfg(target_os = "windows")]
pub use windows::{WindowsInputCapture, WindowsInputInjector};
