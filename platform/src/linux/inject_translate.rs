//! Pure translation from `flow_core`'s [`InputEvent`] back to evdev's raw
//! event model — the inverse of `translate::EventTranslator`, used by
//! [`super::injector::LinuxInputInjector`]. Isolated from any device I/O
//! so it's unit-testable without hardware access (`daemon/todos.json` E2
//! acceptance criteria).

use std::str::FromStr;

use evdev::{EventType, KeyCode, RelativeAxisCode};
use flow_core::protocol::{InputEvent, KeyboardEvent, MouseButton, MouseEvent};

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

/// Reverses `translate::key_name`: `"A"` -> `KeyCode::KEY_A`. Any key name
/// this crate itself produced round-trips; a name from elsewhere that
/// doesn't match a known evdev key code is simply not injectable.
fn key_code_for(key: &str) -> Option<KeyCode> {
    KeyCode::from_str(&format!("KEY_{key}")).ok()
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
