//! Pure(-ish) translation from `flow_core`'s [`InputEvent`] back to a
//! Core Graphics `CGEvent` — the inverse of `translate::EventTranslator`,
//! used by [`super::injector::MacosInputInjector`]. "Pure-ish" because
//! building a `CGEvent` is itself a Core Graphics API call, not just
//! struct construction, but it needs no event tap and no permission, so
//! it's unit-testable the same way `translate.rs` is
//! (`daemon/todos.json` E5 acceptance criteria) — and, like `translate.rs`,
//! these tests can only actually execute on macOS.

use core_graphics::event::{
    CGEvent, CGEventType, CGKeyCode, CGMouseButton, EventField, KeyCode, ScrollEventUnit,
};
use core_graphics::event_source::CGEventSource;
use core_graphics::geometry::{CGPoint, CG_ZERO_POINT};
use flow_core::protocol::{InputEvent, KeyboardEvent, MouseButton, MouseEvent};

/// Translates one `InputEvent` into the `CGEvent` that reproduces it.
/// Returns `None` when there's nothing to post: an unrecognized key name,
/// or the underlying `CGEvent` constructor itself failing (Core Graphics
/// reports that as an opaque `Err(())`, not a reason).
pub fn to_cg_event(source: &CGEventSource, event: &InputEvent) -> Option<CGEvent> {
    match event {
        InputEvent::Keyboard(keyboard_event) => keyboard_to_cg_event(source, keyboard_event),
        InputEvent::Mouse(mouse_event) => mouse_to_cg_event(source, mouse_event),
    }
}

fn keyboard_to_cg_event(source: &CGEventSource, event: &KeyboardEvent) -> Option<CGEvent> {
    let (key, is_down) = match event {
        KeyboardEvent::KeyDown { key, .. } => (key, true),
        KeyboardEvent::KeyUp { key, .. } => (key, false),
    };
    let code = code_for_name(key)?;
    CGEvent::new_keyboard_event(source.clone(), code, is_down).ok()
}

fn mouse_to_cg_event(source: &CGEventSource, event: &MouseEvent) -> Option<CGEvent> {
    match event {
        MouseEvent::Move { dx, dy, .. } => move_cg_event(source, *dx, *dy),
        MouseEvent::Scroll { dx, dy, .. } => scroll_cg_event(source, *dx, *dy),
        MouseEvent::ButtonDown { button, .. } => button_cg_event(source, *button, true),
        MouseEvent::ButtonUp { button, .. } => button_cg_event(source, *button, false),
    }
}

/// `MouseEvent::Move` carries a relative delta, but `CGEvent::new_mouse_event`
/// wants an absolute cursor position — this daemon doesn't track one of
/// its own, so the event is anchored at wherever the cursor actually is
/// right now (`current_location`) and the delta is layered on top via
/// the `MOUSE_EVENT_DELTA_X`/`Y` fields, which is what `CGEventPost`
/// honors for relative motion. Deliberate design choice, not hidden —
/// see `daemon/todos.json` E5's `buildNote`.
fn move_cg_event(source: &CGEventSource, dx: i32, dy: i32) -> Option<CGEvent> {
    let location = current_location(source);
    let event = CGEvent::new_mouse_event(
        source.clone(),
        CGEventType::MouseMoved,
        location,
        CGMouseButton::Left,
    )
    .ok()?;
    event.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_X, dx as i64);
    event.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y, dy as i64);
    Some(event)
}

fn scroll_cg_event(source: &CGEventSource, dx: i32, dy: i32) -> Option<CGEvent> {
    CGEvent::new_scroll_event(source.clone(), ScrollEventUnit::LINE, 2, dy, dx, 0).ok()
}

fn button_cg_event(source: &CGEventSource, button: MouseButton, is_down: bool) -> Option<CGEvent> {
    let (event_type, cg_button) = match (button, is_down) {
        (MouseButton::Left, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left),
        (MouseButton::Left, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left),
        (MouseButton::Right, true) => (CGEventType::RightMouseDown, CGMouseButton::Right),
        (MouseButton::Right, false) => (CGEventType::RightMouseUp, CGMouseButton::Right),
        (MouseButton::Middle, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center),
        (MouseButton::Middle, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center),
    };
    let location = current_location(source);
    let event = CGEvent::new_mouse_event(source.clone(), event_type, location, cg_button).ok()?;
    if button == MouseButton::Middle {
        // OtherMouseDown/Up alone doesn't disambiguate which extra
        // button — the capture side reads this same field to recognize
        // "middle" (translate.rs's `other_button_event`).
        event.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, 2);
    }
    Some(event)
}

fn current_location(source: &CGEventSource) -> CGPoint {
    CGEvent::new(source.clone())
        .map(|event| event.location())
        .unwrap_or(CG_ZERO_POINT)
}

/// Reverses `translate::key_name`: `"RETURN"` -> `KeyCode::RETURN`,
/// `"0x00"` -> `0x00`. Any name this crate itself produced round-trips.
fn code_for_name(name: &str) -> Option<CGKeyCode> {
    Some(match name {
        "RETURN" => KeyCode::RETURN,
        "TAB" => KeyCode::TAB,
        "SPACE" => KeyCode::SPACE,
        "DELETE" => KeyCode::DELETE,
        "ESCAPE" => KeyCode::ESCAPE,
        "COMMAND" => KeyCode::COMMAND,
        "SHIFT" => KeyCode::SHIFT,
        "CAPS_LOCK" => KeyCode::CAPS_LOCK,
        "OPTION" => KeyCode::OPTION,
        "CONTROL" => KeyCode::CONTROL,
        "RIGHT_COMMAND" => KeyCode::RIGHT_COMMAND,
        "RIGHT_SHIFT" => KeyCode::RIGHT_SHIFT,
        "RIGHT_OPTION" => KeyCode::RIGHT_OPTION,
        "RIGHT_CONTROL" => KeyCode::RIGHT_CONTROL,
        "FUNCTION" => KeyCode::FUNCTION,
        "VOLUME_UP" => KeyCode::VOLUME_UP,
        "VOLUME_DOWN" => KeyCode::VOLUME_DOWN,
        "MUTE" => KeyCode::MUTE,
        "F1" => KeyCode::F1,
        "F2" => KeyCode::F2,
        "F3" => KeyCode::F3,
        "F4" => KeyCode::F4,
        "F5" => KeyCode::F5,
        "F6" => KeyCode::F6,
        "F7" => KeyCode::F7,
        "F8" => KeyCode::F8,
        "F9" => KeyCode::F9,
        "F10" => KeyCode::F10,
        "F11" => KeyCode::F11,
        "F12" => KeyCode::F12,
        "F13" => KeyCode::F13,
        "F14" => KeyCode::F14,
        "F15" => KeyCode::F15,
        "F16" => KeyCode::F16,
        "F17" => KeyCode::F17,
        "F18" => KeyCode::F18,
        "F19" => KeyCode::F19,
        "F20" => KeyCode::F20,
        "HELP" => KeyCode::HELP,
        "HOME" => KeyCode::HOME,
        "PAGE_UP" => KeyCode::PAGE_UP,
        "FORWARD_DELETE" => KeyCode::FORWARD_DELETE,
        "END" => KeyCode::END,
        "PAGE_DOWN" => KeyCode::PAGE_DOWN,
        "LEFT_ARROW" => KeyCode::LEFT_ARROW,
        "RIGHT_ARROW" => KeyCode::RIGHT_ARROW,
        "DOWN_ARROW" => KeyCode::DOWN_ARROW,
        "UP_ARROW" => KeyCode::UP_ARROW,
        hex => {
            return hex
                .strip_prefix("0x")
                .and_then(|digits| u16::from_str_radix(digits, 16).ok())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_graphics::event_source::CGEventSourceStateID;

    fn source() -> CGEventSource {
        CGEventSource::new(CGEventSourceStateID::HIDSystemState).expect("event source")
    }

    #[test]
    fn a_named_key_round_trips_through_the_capture_sides_name() {
        let event = to_cg_event(
            &source(),
            &InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "RETURN".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            }),
        )
        .unwrap();
        assert_eq!(event.get_type() as u32, CGEventType::KeyDown as u32);
        assert_eq!(
            event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE),
            KeyCode::RETURN as i64
        );
    }

    #[test]
    fn a_hex_fallback_key_round_trips_to_its_raw_code() {
        let event = to_cg_event(
            &source(),
            &InputEvent::Keyboard(KeyboardEvent::KeyUp {
                key: "0x00".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            }),
        )
        .unwrap();
        assert_eq!(
            event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE),
            0
        );
    }

    #[test]
    fn an_unknown_key_name_translates_to_nothing() {
        assert!(to_cg_event(
            &source(),
            &InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "NOT_A_REAL_KEY".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            })
        )
        .is_none());
    }

    #[test]
    fn mouse_move_carries_its_delta_in_the_delta_fields() {
        let event = to_cg_event(
            &source(),
            &InputEvent::Mouse(MouseEvent::Move {
                dx: 5,
                dy: -3,
                timestamp_ms: 0,
            }),
        )
        .unwrap();
        assert_eq!(event.get_type() as u32, CGEventType::MouseMoved as u32);
        assert_eq!(
            event.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_X),
            5
        );
        assert_eq!(
            event.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y),
            -3
        );
    }

    #[test]
    fn scroll_maps_dy_to_axis_one_and_dx_to_axis_two() {
        let event = to_cg_event(
            &source(),
            &InputEvent::Mouse(MouseEvent::Scroll {
                dx: -1,
                dy: 2,
                timestamp_ms: 0,
            }),
        )
        .unwrap();
        assert_eq!(event.get_type() as u32, CGEventType::ScrollWheel as u32);
        assert_eq!(
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1),
            2
        );
        assert_eq!(
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2),
            -1
        );
    }

    #[test]
    fn left_and_right_buttons_map_to_their_dedicated_event_types() {
        let down = to_cg_event(
            &source(),
            &InputEvent::Mouse(MouseEvent::ButtonDown {
                button: MouseButton::Right,
                timestamp_ms: 0,
            }),
        )
        .unwrap();
        assert_eq!(down.get_type() as u32, CGEventType::RightMouseDown as u32);

        let up = to_cg_event(
            &source(),
            &InputEvent::Mouse(MouseEvent::ButtonUp {
                button: MouseButton::Left,
                timestamp_ms: 0,
            }),
        )
        .unwrap();
        assert_eq!(up.get_type() as u32, CGEventType::LeftMouseUp as u32);
    }

    #[test]
    fn the_middle_button_uses_other_mouse_events_with_button_number_two() {
        let event = to_cg_event(
            &source(),
            &InputEvent::Mouse(MouseEvent::ButtonDown {
                button: MouseButton::Middle,
                timestamp_ms: 0,
            }),
        )
        .unwrap();
        assert_eq!(event.get_type() as u32, CGEventType::OtherMouseDown as u32);
        assert_eq!(
            event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER),
            2
        );
    }
}
