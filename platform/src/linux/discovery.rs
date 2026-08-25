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

/// Every currently connected device evdev can open that supports at least
/// one key or relative-axis event.
pub fn discover_devices() -> Vec<Device> {
    evdev::enumerate()
        .map(|(_path, device)| device)
        .filter(is_keyboard_or_mouse)
        .collect()
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
