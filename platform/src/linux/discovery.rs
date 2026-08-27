//! Picks which of the kernel's `/dev/input/event*` nodes to read from.
//!
//! `evdev::enumerate()` already skips nodes it can't open (permission
//! denied, or `/dev/input` missing entirely), so this only adds Flow's own
//! filter on top: keep devices that report keyboard keys and/or relative
//! mouse axes, the two event classes `super::translate::EventTranslator`
//! understands. This is a heuristic, not a precise "is this a keyboard"
//! check (a device exposing a handful of media keys still qualifies) — see
//! `daemon/todos.json` E1's `buildNote`.

use evdev::Device;

/// The uinput device `super::injector::LinuxInputInjector` creates. Never
/// captured from: it's this daemon's *own* injected output, so reading it
/// back would relay remote input straight onto another peer (and, with
/// two peer connections live at once, loop it between them). Shared with
/// the injector, which declares the device under this exact name, so the
/// filter can't silently drift away from what's actually created.
pub(super) const VIRTUAL_DEVICE_NAME: &str = "Flow Virtual Input";

/// Every currently connected device evdev can open that supports at least
/// one key or relative-axis event, excluding Flow's own virtual output
/// device.
pub fn discover_devices() -> Vec<Device> {
    evdev::enumerate()
        .map(|(_path, device)| device)
        .filter(|device| !is_flow_virtual_device(device))
        .filter(is_keyboard_or_mouse)
        .collect()
}

/// Whether this is the virtual device Flow itself injects through.
fn is_flow_virtual_device(device: &Device) -> bool {
    device.name() == Some(VIRTUAL_DEVICE_NAME)
}

fn is_keyboard_or_mouse(device: &Device) -> bool {
    let has_keys = device
        .supported_keys()
        .is_some_and(|keys| keys.iter().next().is_some());
    let has_relative_axes = device
        .supported_relative_axes()
        .is_some_and(|axes| axes.iter().next().is_some());
    has_keys || has_relative_axes
}
