//! OS-specific input capture/injection adapters (vision.md §6-§7).
//!
//! Exactly one of these modules compiles for a given build target. Code
//! outside this crate depends only on the traits in `flow_core::input`
//! and, where it does need a concrete type to construct (the daemon's
//! hotkey runner, track F2), the [`DefaultInputCapture`] alias below —
//! never a per-OS type name directly — so this crate stays the only
//! place that needs to know which operating system it's running on.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::{LinuxInputCapture, LinuxInputInjector};
#[cfg(target_os = "macos")]
pub use macos::{MacosCaptureError, MacosInjectError, MacosInputCapture, MacosInputInjector};
#[cfg(target_os = "windows")]
pub use windows::{
    WindowsCaptureError, WindowsInjectError, WindowsInputCapture, WindowsInputInjector,
};

/// The `InputCapture` implementation for whichever OS this crate is
/// built for. Every adapter shares the same `new(sender)` constructor
/// shape by convention (not a trait — the trait itself has no
/// constructor, since platforms differ in what they need up front), so
/// callers that just want "the real one for this machine" can name this
/// alias instead of writing their own `#[cfg(target_os = ...)]` chain.
#[cfg(target_os = "linux")]
pub type DefaultInputCapture = LinuxInputCapture;
#[cfg(target_os = "macos")]
pub type DefaultInputCapture = MacosInputCapture;
#[cfg(target_os = "windows")]
pub type DefaultInputCapture = WindowsInputCapture;
