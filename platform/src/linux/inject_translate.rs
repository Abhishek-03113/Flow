//! Pure translation from `flow_core`'s [`InputEvent`] back to evdev's raw
//! event model — the inverse of `translate::EventTranslator`, used by
//! [`super::injector::LinuxInputInjector`]. Isolated from any device I/O
//! so it's unit-testable without hardware access (`docs/tasks/daemon-todos.json` E2
//! acceptance criteria).

use std::str::FromStr;

use evdev::{EventType, KeyCode, RelativeAxisCode};
use flow_core::protocol::{key_names, InputEvent, KeyboardEvent, MouseButton, MouseEvent};

use super::translate::{KEY_DOWN, KEY_UP};

/// Translates one `InputEvent` into the raw evdev events that reproduce it
/// on a virtual device. Returns `None` when there's nothing to emit: an
/// unrecognized key name, or a `Move`/`Scroll` whose deltas are both zero.
pub fn to_uinput_events(event: &InputEvent) -> Option<Vec<evdev::InputEvent>> {
    match event {
        InputEvent::Keyboard(keyboard_event) => {
            keyboard_to_uinput(keyboard_event).map(|event| vec![event])
        }
        InputEvent::Mouse(mouse_event) => mouse_to_uinput(mouse_event),
    }
}

fn keyboard_to_uinput(event: &KeyboardEvent) -> Option<evdev::InputEvent> {
    let (key, value) = match event {
        KeyboardEvent::KeyDown { key, .. } => (key, KEY_DOWN),
        KeyboardEvent::KeyUp { key, .. } => (key, KEY_UP),
    };
    let code = key_code_for(key)?;
    Some(evdev::InputEvent::new(EventType::KEY.0, code.0, value))
}

fn mouse_to_uinput(event: &MouseEvent) -> Option<Vec<evdev::InputEvent>> {
    match event {
        MouseEvent::Move { dx, dy, .. } => relative_axis_events([
            (RelativeAxisCode::REL_X, *dx),
            (RelativeAxisCode::REL_Y, *dy),
        ]),
        MouseEvent::Scroll { dx, dy, .. } => relative_axis_events([
            (RelativeAxisCode::REL_WHEEL, *dy),
            (RelativeAxisCode::REL_HWHEEL, *dx),
        ]),
        MouseEvent::ButtonDown { button, .. } => Some(vec![button_event(*button, KEY_DOWN)]),
        MouseEvent::ButtonUp { button, .. } => Some(vec![button_event(*button, KEY_UP)]),
    }
}

fn relative_axis_events(axes: [(RelativeAxisCode, i32); 2]) -> Option<Vec<evdev::InputEvent>> {
    let events: Vec<_> = axes
        .into_iter()
        .filter(|(_, value)| *value != 0)
        .map(|(code, value)| evdev::InputEvent::new(EventType::RELATIVE.0, code.0, value))
        .collect();
    (!events.is_empty()).then_some(events)
}

fn button_event(button: MouseButton, value: i32) -> evdev::InputEvent {
    let code = match button {
        MouseButton::Left => KeyCode::BTN_LEFT,
        MouseButton::Right => KeyCode::BTN_RIGHT,
        MouseButton::Middle => KeyCode::BTN_MIDDLE,
    };
    evdev::InputEvent::new(EventType::KEY.0, code.0, value)
}

/// The canonical-name half of `key_code_for`'s reverse mapping — must be
/// checked before the generic passthrough, since a shared name like
/// `"RETURN"` does not match evdev's own `KEY_ENTER` by naive
/// `KEY_`-prefixing.
fn canonical_code_for(name: &str) -> Option<KeyCode> {
    Some(match name {
        n if n == key_names::RETURN => KeyCode::KEY_ENTER,
        n if n == key_names::TAB => KeyCode::KEY_TAB,
        n if n == key_names::SPACE => KeyCode::KEY_SPACE,
        n if n == key_names::DELETE => KeyCode::KEY_BACKSPACE,
        n if n == key_names::FORWARD_DELETE => KeyCode::KEY_DELETE,
        n if n == key_names::ESCAPE => KeyCode::KEY_ESC,
        n if n == key_names::SHIFT => KeyCode::KEY_LEFTSHIFT,
        n if n == key_names::RIGHT_SHIFT => KeyCode::KEY_RIGHTSHIFT,
        n if n == key_names::CONTROL => KeyCode::KEY_LEFTCTRL,
        n if n == key_names::RIGHT_CONTROL => KeyCode::KEY_RIGHTCTRL,
        n if n == key_names::OPTION => KeyCode::KEY_LEFTALT,
        n if n == key_names::RIGHT_OPTION => KeyCode::KEY_RIGHTALT,
        n if n == key_names::COMMAND => KeyCode::KEY_LEFTMETA,
        n if n == key_names::RIGHT_COMMAND => KeyCode::KEY_RIGHTMETA,
        n if n == key_names::CAPS_LOCK => KeyCode::KEY_CAPSLOCK,
        n if n == key_names::FUNCTION => KeyCode::KEY_FN,
        n if n == key_names::HOME => KeyCode::KEY_HOME,
        n if n == key_names::END => KeyCode::KEY_END,
        n if n == key_names::PAGE_UP => KeyCode::KEY_PAGEUP,
        n if n == key_names::PAGE_DOWN => KeyCode::KEY_PAGEDOWN,
        n if n == key_names::LEFT_ARROW => KeyCode::KEY_LEFT,
        n if n == key_names::RIGHT_ARROW => KeyCode::KEY_RIGHT,
        n if n == key_names::UP_ARROW => KeyCode::KEY_UP,
        n if n == key_names::DOWN_ARROW => KeyCode::KEY_DOWN,
        n if n == key_names::HELP => KeyCode::KEY_HELP,
        n if n == key_names::VOLUME_UP => KeyCode::KEY_VOLUMEUP,
        n if n == key_names::VOLUME_DOWN => KeyCode::KEY_VOLUMEDOWN,
        n if n == key_names::MUTE => KeyCode::KEY_MUTE,
        n if n == key_names::MINUS => KeyCode::KEY_MINUS,
        n if n == key_names::EQUAL => KeyCode::KEY_EQUAL,
        n if n == key_names::LEFT_BRACKET => KeyCode::KEY_LEFTBRACE,
        n if n == key_names::RIGHT_BRACKET => KeyCode::KEY_RIGHTBRACE,
        n if n == key_names::SEMICOLON => KeyCode::KEY_SEMICOLON,
        n if n == key_names::QUOTE => KeyCode::KEY_APOSTROPHE,
        n if n == key_names::COMMA => KeyCode::KEY_COMMA,
        n if n == key_names::PERIOD => KeyCode::KEY_DOT,
        n if n == key_names::SLASH => KeyCode::KEY_SLASH,
        n if n == key_names::BACKSLASH => KeyCode::KEY_BACKSLASH,
        n if n == key_names::GRAVE => KeyCode::KEY_GRAVE,
        n if n == key_names::function_key(1) => KeyCode::KEY_F1,
        n if n == key_names::function_key(2) => KeyCode::KEY_F2,
        n if n == key_names::function_key(3) => KeyCode::KEY_F3,
        n if n == key_names::function_key(4) => KeyCode::KEY_F4,
        n if n == key_names::function_key(5) => KeyCode::KEY_F5,
        n if n == key_names::function_key(6) => KeyCode::KEY_F6,
        n if n == key_names::function_key(7) => KeyCode::KEY_F7,
        n if n == key_names::function_key(8) => KeyCode::KEY_F8,
        n if n == key_names::function_key(9) => KeyCode::KEY_F9,
        n if n == key_names::function_key(10) => KeyCode::KEY_F10,
        n if n == key_names::function_key(11) => KeyCode::KEY_F11,
        n if n == key_names::function_key(12) => KeyCode::KEY_F12,
        n if n == key_names::function_key(13) => KeyCode::KEY_F13,
        n if n == key_names::function_key(14) => KeyCode::KEY_F14,
        n if n == key_names::function_key(15) => KeyCode::KEY_F15,
        n if n == key_names::function_key(16) => KeyCode::KEY_F16,
        n if n == key_names::function_key(17) => KeyCode::KEY_F17,
        n if n == key_names::function_key(18) => KeyCode::KEY_F18,
        n if n == key_names::function_key(19) => KeyCode::KEY_F19,
        n if n == key_names::function_key(20) => KeyCode::KEY_F20,
        _ => return None,
    })
}

/// Reverses `translate::key_name`: `"A"` -> `KeyCode::KEY_A`, `"RETURN"`
/// -> `KeyCode::KEY_ENTER`. Any key name this crate itself produced
/// round-trips; a name from elsewhere that doesn't match a shared
/// canonical name or a known evdev key code is simply not injectable.
fn key_code_for(key: &str) -> Option<KeyCode> {
    canonical_code_for(key).or_else(|| KeyCode::from_str(&format!("KEY_{key}")).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow_core::protocol::Modifier;

    #[test]
    fn key_down_and_up_round_trip_through_the_capture_side_name() {
        let down = to_uinput_events(&InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: "A".to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(
            down,
            vec![evdev::InputEvent::new(
                EventType::KEY.0,
                KeyCode::KEY_A.0,
                1
            )]
        );

        let up = to_uinput_events(&InputEvent::Keyboard(KeyboardEvent::KeyUp {
            key: "A".to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(
            up,
            vec![evdev::InputEvent::new(
                EventType::KEY.0,
                KeyCode::KEY_A.0,
                0
            )]
        );
    }

    #[test]
    fn a_modifier_keys_own_name_also_round_trips() {
        let down = to_uinput_events(&InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: "LEFTSHIFT".to_string(),
            modifiers: vec![Modifier::Shift],
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(
            down,
            vec![evdev::InputEvent::new(
                EventType::KEY.0,
                KeyCode::KEY_LEFTSHIFT.0,
                1
            )]
        );
    }

    #[test]
    fn an_unknown_key_name_translates_to_nothing() {
        assert!(
            to_uinput_events(&InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "NOT_A_REAL_KEY".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            }))
            .is_none()
        );
    }

    #[test]
    fn every_shared_key_name_has_a_linux_target() {
        for name in flow_core::protocol::key_names::all() {
            assert!(
                key_code_for(&name).is_some(),
                "no evdev KeyCode for shared key name {name:?}"
            );
        }
    }

    #[test]
    fn mouse_move_emits_only_nonzero_axes() {
        let events = to_uinput_events(&InputEvent::Mouse(MouseEvent::Move {
            dx: 5,
            dy: 0,
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(
            events,
            vec![evdev::InputEvent::new(
                EventType::RELATIVE.0,
                RelativeAxisCode::REL_X.0,
                5
            )]
        );
    }

    #[test]
    fn a_zero_delta_move_translates_to_nothing() {
        assert!(to_uinput_events(&InputEvent::Mouse(MouseEvent::Move {
            dx: 0,
            dy: 0,
            timestamp_ms: 0,
        }))
        .is_none());
    }

    #[test]
    fn scroll_maps_dy_to_wheel_and_dx_to_hwheel() {
        let events = to_uinput_events(&InputEvent::Mouse(MouseEvent::Scroll {
            dx: -1,
            dy: 2,
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(
            events,
            vec![
                evdev::InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_WHEEL.0, 2),
                evdev::InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_HWHEEL.0, -1),
            ]
        );
    }

    #[test]
    fn mouse_buttons_map_to_their_btn_codes() {
        let down = to_uinput_events(&InputEvent::Mouse(MouseEvent::ButtonDown {
            button: MouseButton::Middle,
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(
            down,
            vec![evdev::InputEvent::new(
                EventType::KEY.0,
                KeyCode::BTN_MIDDLE.0,
                1
            )]
        );
    }
}
