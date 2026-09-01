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

/// Which mouse buttons are currently held down on the receiving side.
///
/// [`super::injector::MacosInputInjector`] keeps one of these across
/// injected events and hands it to [`to_cg_event`] so a `MouseEvent::Move`
/// that arrives between a `ButtonDown` and its `ButtonUp` is posted as the
/// matching `*MouseDragged` event rather than a plain `MouseMoved` —
/// AppKit text selection and drag-and-drop only follow motion that
/// carries the pressed button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeldButtons {
    left: bool,
    right: bool,
    middle: bool,
}

impl HeldButtons {
    /// Records `button` as pressed.
    pub fn press(&mut self, button: MouseButton) {
        self.set(button, true);
    }

    /// Records `button` as released.
    pub fn release(&mut self, button: MouseButton) {
        self.set(button, false);
    }

    fn set(&mut self, button: MouseButton, down: bool) {
        match button {
            MouseButton::Left => self.left = down,
            MouseButton::Right => self.right = down,
            MouseButton::Middle => self.middle = down,
        }
    }

    /// The button a mid-press move should be attributed to, or `None`
    /// when nothing is held (an ordinary `MouseMoved`). Left wins over
    /// right over middle when several are held at once — the capture side
    /// doesn't produce simultaneous multi-button drags in practice, so
    /// any stable order is fine.
    fn drag_button(&self) -> Option<MouseButton> {
        if self.left {
            Some(MouseButton::Left)
        } else if self.right {
            Some(MouseButton::Right)
        } else if self.middle {
            Some(MouseButton::Middle)
        } else {
            None
        }
    }
}

/// Translates one `InputEvent` into the `CGEvent` that reproduces it.
/// Returns `None` when there's nothing to post: an unrecognized key name,
/// or the underlying `CGEvent` constructor itself failing (Core Graphics
/// reports that as an opaque `Err(())`, not a reason).
///
/// `held` is the set of mouse buttons currently down on this machine; it
/// only affects `MouseEvent::Move` translation (drag vs. plain move).
pub fn to_cg_event(
    source: &CGEventSource,
    event: &InputEvent,
    held: HeldButtons,
) -> Option<CGEvent> {
    match event {
        InputEvent::Keyboard(keyboard_event) => keyboard_to_cg_event(source, keyboard_event),
        InputEvent::Mouse(mouse_event) => mouse_to_cg_event(source, mouse_event, held),
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

fn mouse_to_cg_event(
    source: &CGEventSource,
    event: &MouseEvent,
    held: HeldButtons,
) -> Option<CGEvent> {
    match event {
        MouseEvent::Move { dx, dy, .. } => move_cg_event(source, *dx, *dy, held),
        MouseEvent::Scroll { dx, dy, .. } => scroll_cg_event(source, *dx, *dy),
        MouseEvent::ButtonDown { button, .. } => button_cg_event(source, *button, true),
        MouseEvent::ButtonUp { button, .. } => button_cg_event(source, *button, false),
    }
}

/// `MouseEvent::Move` carries a relative delta, but `CGEvent::new_mouse_event`
/// wants an absolute cursor position, and the window server positions the
/// cursor from that absolute location — the `MOUSE_EVENT_DELTA_X`/`Y`
/// fields alone (the original E5 approach) left the pointer pinned in
/// place. This daemon keeps no cursor position of its own, so the target
/// is computed fresh each event as "wherever the cursor is right now
/// (`current_location`) plus this delta". The delta fields are still set,
/// so apps that read raw deltas (games, pointer-lock) keep working.
///
/// While a mouse button is held the event is posted as the matching
/// `*MouseDragged` type rather than `MouseMoved`; AppKit only treats
/// motion as a drag when it carries the pressed button.
fn move_cg_event(source: &CGEventSource, dx: i32, dy: i32, held: HeldButtons) -> Option<CGEvent> {
    let target = offset(current_location(source), dx, dy);
    let (event_type, cg_button, other_button_number) = match held.drag_button() {
        None => (CGEventType::MouseMoved, CGMouseButton::Left, None),
        Some(MouseButton::Left) => (CGEventType::LeftMouseDragged, CGMouseButton::Left, None),
        Some(MouseButton::Right) => (CGEventType::RightMouseDragged, CGMouseButton::Right, None),
        Some(MouseButton::Middle) => (
            CGEventType::OtherMouseDragged,
            CGMouseButton::Center,
            Some(2),
        ),
    };
    let event = CGEvent::new_mouse_event(source.clone(), event_type, target, cg_button).ok()?;
    event.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_X, dx as i64);
    event.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y, dy as i64);
    if let Some(number) = other_button_number {
        // `OtherMouseDragged` alone doesn't say which extra button — the
        // capture side reads this field back to recognize "middle"
        // (`translate.rs`'s `other_button_event`), same as the button
        // events below.
        event.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, number);
    }
    Some(event)
}

/// `point` shifted by a relative pixel delta, in the global
/// top-left-origin display coordinates `CGEvent` locations use. Core
/// Graphics clamps the posted event to the display bounds, so no manual
/// clamp is needed here.
fn offset(point: CGPoint, dx: i32, dy: i32) -> CGPoint {
    CGPoint::new(point.x + dx as f64, point.y + dy as f64)
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
    // AppKit controls (buttons, menu items, `-mouseDown:` handlers) only
    // accept a synthetic click whose click-state is set; without it the
    // down/up pair is delivered but ignored. Always 1 — this path posts
    // single discrete clicks, never the 2/3 of a double/triple click.
    event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, 1);
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

    /// No mouse button held — the common case for every non-drag test.
    fn no_buttons() -> HeldButtons {
        HeldButtons::default()
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
            no_buttons(),
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
            no_buttons(),
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
            }),
            no_buttons(),
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
            no_buttons(),
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
    fn mouse_move_targets_the_current_cursor_position_plus_the_delta() {
        let src = source();
        let here = current_location(&src);
        let event = to_cg_event(
            &src,
            &InputEvent::Mouse(MouseEvent::Move {
                dx: 7,
                dy: 11,
                timestamp_ms: 0,
            }),
            no_buttons(),
        )
        .unwrap();
        let at = event.location();
        assert!(
            (at.x - (here.x + 7.0)).abs() < 1.0,
            "x: {} vs {}",
            at.x,
            here.x
        );
        assert!(
            (at.y - (here.y + 11.0)).abs() < 1.0,
            "y: {} vs {}",
            at.y,
            here.y
        );
    }

    #[test]
    fn a_move_while_the_left_button_is_held_is_a_left_drag() {
        let mut held = HeldButtons::default();
        held.press(MouseButton::Left);
        let event = to_cg_event(
            &source(),
            &InputEvent::Mouse(MouseEvent::Move {
                dx: 1,
                dy: 1,
                timestamp_ms: 0,
            }),
            held,
        )
        .unwrap();
        assert_eq!(
            event.get_type() as u32,
            CGEventType::LeftMouseDragged as u32
        );
    }

    #[test]
    fn a_move_while_the_middle_button_is_held_is_an_other_drag_with_button_number_two() {
        let mut held = HeldButtons::default();
        held.press(MouseButton::Middle);
        let event = to_cg_event(
            &source(),
            &InputEvent::Mouse(MouseEvent::Move {
                dx: 1,
                dy: 0,
                timestamp_ms: 0,
            }),
            held,
        )
        .unwrap();
        assert_eq!(
            event.get_type() as u32,
            CGEventType::OtherMouseDragged as u32
        );
        assert_eq!(
            event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER),
            2
        );
    }

    #[test]
    fn releasing_the_only_held_button_makes_moves_plain_again() {
        let mut held = HeldButtons::default();
        held.press(MouseButton::Left);
        held.release(MouseButton::Left);
        let event = to_cg_event(
            &source(),
            &InputEvent::Mouse(MouseEvent::Move {
                dx: 1,
                dy: 0,
                timestamp_ms: 0,
            }),
            held,
        )
        .unwrap();
        assert_eq!(event.get_type() as u32, CGEventType::MouseMoved as u32);
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
            no_buttons(),
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
            no_buttons(),
        )
        .unwrap();
        assert_eq!(down.get_type() as u32, CGEventType::RightMouseDown as u32);

        let up = to_cg_event(
            &source(),
            &InputEvent::Mouse(MouseEvent::ButtonUp {
                button: MouseButton::Left,
                timestamp_ms: 0,
            }),
            no_buttons(),
        )
        .unwrap();
        assert_eq!(up.get_type() as u32, CGEventType::LeftMouseUp as u32);
    }

    #[test]
    fn a_click_carries_a_click_state_so_appkit_controls_accept_it() {
        let event = to_cg_event(
            &source(),
            &InputEvent::Mouse(MouseEvent::ButtonDown {
                button: MouseButton::Left,
                timestamp_ms: 0,
            }),
            no_buttons(),
        )
        .unwrap();
        assert_eq!(
            event.get_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE),
            1
        );
    }

    #[test]
    fn the_middle_button_uses_other_mouse_events_with_button_number_two() {
        let event = to_cg_event(
            &source(),
            &InputEvent::Mouse(MouseEvent::ButtonDown {
                button: MouseButton::Middle,
                timestamp_ms: 0,
            }),
            no_buttons(),
        )
        .unwrap();
        assert_eq!(event.get_type() as u32, CGEventType::OtherMouseDown as u32);
        assert_eq!(
            event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER),
            2
        );
    }
}
