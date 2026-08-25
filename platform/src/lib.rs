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

/// The `InputInjector` implementation for whichever OS this crate is
/// built for — the injection counterpart to [`DefaultInputCapture`].
#[cfg(target_os = "linux")]
pub type DefaultInputInjector = LinuxInputInjector;
#[cfg(target_os = "macos")]
pub type DefaultInputInjector = MacosInputInjector;
#[cfg(target_os = "windows")]
pub type DefaultInputInjector = WindowsInputInjector;

/// Constructs [`DefaultInputInjector`], normalizing each platform's own
/// constructor shape into one fallible call: Linux/macOS's injectors are
/// fallible (`new() -> Result<Self, _>`, since building the virtual
/// device/event tap can fail), Windows's has no setup to fail (`SendInput`
/// needs no handle up front, so the struct is just `Default`). Unlike
/// [`DefaultInputCapture`] (whose `new(sender)` shape genuinely is
/// uniform across all three platforms), the injector's constructors
/// aren't — so, rather than let that leak into every caller as a
/// per-platform match, this function is the one place that absorbs the
/// difference, keeping this crate "the only place that needs to know
/// which operating system it's running on."
pub fn new_default_input_injector() -> Result<DefaultInputInjector, Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        DefaultInputInjector::new().map_err(Into::into)
    }
    #[cfg(target_os = "macos")]
    {
        DefaultInputInjector::new().map_err(Into::into)
    }
    #[cfg(target_os = "windows")]
    {
        Ok(DefaultInputInjector::default())
    }
}
