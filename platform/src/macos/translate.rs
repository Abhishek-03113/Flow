//! Pure translation from Core Graphics' `CGEvent` model to `flow_core`'s
//! platform-independent [`InputEvent`] (vision.md §11). Isolated from any
//! event tap so it's unit-testable without installing one
//! (`daemon/todos.json` E4 acceptance criteria) — though, like the rest of
//! this module, the tests here can only actually run on macOS; this
//! session verified them by cross-compiling only (`daemon/README.md`).

use std::collections::HashSet;

use core_graphics::event::{CGEvent, CGEventFlags, CGEventType, CGKeyCode, EventField, KeyCode};
use flow_core::protocol::{InputEvent, KeyboardEvent, Modifier, MouseButton, MouseEvent};

/// Converts `CGEvent`s into `flow_core` [`InputEvent`]s.
///
/// Stateful for two reasons a single event alone can't cover: modifier
/// keys report through `FlagsChanged` (a flag bitmask, not a discrete
/// press/release) rather than `KeyDown`/`KeyUp`, so held state has to be
/// diffed across events; and that same held state is what lets every
/// keyboard event carry the full modifier list `docs/contracts/data-model.md`
/// expects.
#[derive(Debug, Default)]
pub struct EventTranslator {
    held_modifiers: HashSet<Modifier>,
}

impl EventTranslator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Translates one `CGEvent`. `timestamp_ms` is supplied by the caller
    /// (rather than read off the event) so this stays a pure function of
    /// its arguments — safe polling, event-tap install, and wall-clock
    /// reads all stay in `capture.rs`. Returns `None` for event kinds
    /// Flow doesn't model (tablet events, the tap-disabled notifications,
    /// ...).
    pub fn translate(
        &mut self,
        event_type: CGEventType,
        event: &CGEvent,
        timestamp_ms: u64,
    ) -> Option<InputEvent> {
        match event_type {
            CGEventType::KeyDown => Some(self.keyboard_event(event, true, timestamp_ms)),
            CGEventType::KeyUp => Some(self.keyboard_event(event, false, timestamp_ms)),
            CGEventType::FlagsChanged => self.translate_flags_changed(event, timestamp_ms),
            CGEventType::LeftMouseDown => Some(button_event(MouseButton::Left, true, timestamp_ms)),
            CGEventType::LeftMouseUp => Some(button_event(MouseButton::Left, false, timestamp_ms)),
            CGEventType::RightMouseDown => {
                Some(button_event(MouseButton::Right, true, timestamp_ms))
            }
            CGEventType::RightMouseUp => {
                Some(button_event(MouseButton::Right, false, timestamp_ms))
            }
            CGEventType::OtherMouseDown => other_button_event(event, true, timestamp_ms),
            CGEventType::OtherMouseUp => other_button_event(event, false, timestamp_ms),
            CGEventType::MouseMoved
            | CGEventType::LeftMouseDragged
            | CGEventType::RightMouseDragged
            | CGEventType::OtherMouseDragged => Some(move_event(event, timestamp_ms)),
            CGEventType::ScrollWheel => Some(scroll_event(event, timestamp_ms)),
            _ => None,
        }
    }

    fn keyboard_event(&self, event: &CGEvent, is_down: bool, timestamp_ms: u64) -> InputEvent {
        let code = keycode_of(event);
        let key = key_name(code);
        let modifiers = self.modifiers_snapshot();
        InputEvent::Keyboard(if is_down {
            KeyboardEvent::KeyDown {
                key,
                modifiers,
                timestamp_ms,
            }
        } else {
            KeyboardEvent::KeyUp {
                key,
                modifiers,
                timestamp_ms,
            }
        })
    }

    /// `FlagsChanged` carries the physical key that just changed (in the
    /// same keycode field as `KeyDown`/`KeyUp`) plus the *resulting* flag
    /// bitmask — so whether this was a press or a release for that
    /// specific modifier is a diff against what was already held, not
    /// something the event states directly.
    fn translate_flags_changed(
        &mut self,
        event: &CGEvent,
        timestamp_ms: u64,
    ) -> Option<InputEvent> {
        let code = keycode_of(event);
        let modifier = modifier_for(code)?;
        let now_held = event.get_flags().contains(flag_bit_for(modifier));
        let was_held = self.held_modifiers.contains(&modifier);
        if now_held == was_held {
            return None;
        }
        if now_held {
            self.held_modifiers.insert(modifier);
        } else {
            self.held_modifiers.remove(&modifier);
        }
        let key = key_name(code);
        let modifiers = self.modifiers_snapshot();
        Some(InputEvent::Keyboard(if now_held {
            KeyboardEvent::KeyDown {
                key,
                modifiers,
                timestamp_ms,
            }
        } else {
            KeyboardEvent::KeyUp {
                key,
                modifiers,
                timestamp_ms,
            }
        }))
    }

    /// Modifiers currently held, in a fixed order so callers (including
    /// tests) see a deterministic list regardless of press order.
    fn modifiers_snapshot(&self) -> Vec<Modifier> {
        const ORDER: [Modifier; 4] = [
            Modifier::Shift,
            Modifier::Ctrl,
            Modifier::Alt,
            Modifier::Meta,
        ];
        ORDER
            .into_iter()
            .filter(|modifier| self.held_modifiers.contains(modifier))
            .collect()
    }
}

fn keycode_of(event: &CGEvent) -> CGKeyCode {
    event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as CGKeyCode
}

fn modifier_for(code: CGKeyCode) -> Option<Modifier> {
    match code {
        KeyCode::SHIFT | KeyCode::RIGHT_SHIFT => Some(Modifier::Shift),
        KeyCode::CONTROL | KeyCode::RIGHT_CONTROL => Some(Modifier::Ctrl),
        KeyCode::OPTION | KeyCode::RIGHT_OPTION => Some(Modifier::Alt),
        KeyCode::COMMAND | KeyCode::RIGHT_COMMAND => Some(Modifier::Meta),
        _ => None,
    }
}

fn flag_bit_for(modifier: Modifier) -> CGEventFlags {
    match modifier {
        Modifier::Shift => CGEventFlags::CGEventFlagShift,
        Modifier::Ctrl => CGEventFlags::CGEventFlagControl,
        Modifier::Alt => CGEventFlags::CGEventFlagAlternate,
        Modifier::Meta => CGEventFlags::CGEventFlagCommand,
    }
}

fn button_event(button: MouseButton, is_down: bool, timestamp_ms: u64) -> InputEvent {
    InputEvent::Mouse(if is_down {
        MouseEvent::ButtonDown {
            button,
            timestamp_ms,
        }
    } else {
        MouseEvent::ButtonUp {
            button,
            timestamp_ms,
        }
    })
}

/// `OtherMouseDown`/`Up` cover every button beyond left/right; Flow only
/// models a middle button (`MOUSE_EVENT_BUTTON_NUMBER` `2`, per Apple's
/// numbering) — anything past that (a 4th/5th side button) has no
/// contract representation and is dropped.
fn other_button_event(event: &CGEvent, is_down: bool, timestamp_ms: u64) -> Option<InputEvent> {
    const MIDDLE_BUTTON_NUMBER: i64 = 2;
    if event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER) == MIDDLE_BUTTON_NUMBER
    {
        Some(button_event(MouseButton::Middle, is_down, timestamp_ms))
    } else {
        None
    }
}

fn move_event(event: &CGEvent, timestamp_ms: u64) -> InputEvent {
    let dx = event.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_X) as i32;
    let dy = event.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y) as i32;
    InputEvent::Mouse(MouseEvent::Move {
        dx,
        dy,
        timestamp_ms,
    })
}

fn scroll_event(event: &CGEvent, timestamp_ms: u64) -> InputEvent {
    let dy = event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1) as i32;
    let dx = event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2) as i32;
    InputEvent::Mouse(MouseEvent::Scroll {
        dx,
        dy,
        timestamp_ms,
    })
}

/// Names a virtual keycode the same way `flow-platform`'s Linux side
/// names an evdev `KeyCode`: a short, human-readable token, not the raw
/// number. macOS's virtual keycodes are a fixed hardware-position table
/// (unlike evdev's named constants for every key), so this only names
/// the codes `core_graphics::event::KeyCode` itself names; anything else
/// falls back to a hex literal.
fn key_name(code: CGKeyCode) -> String {
    match code {
        KeyCode::RETURN => "RETURN".to_string(),
        KeyCode::TAB => "TAB".to_string(),
        KeyCode::SPACE => "SPACE".to_string(),
        KeyCode::DELETE => "DELETE".to_string(),
        KeyCode::ESCAPE => "ESCAPE".to_string(),
        KeyCode::COMMAND => "COMMAND".to_string(),
        KeyCode::SHIFT => "SHIFT".to_string(),
        KeyCode::CAPS_LOCK => "CAPS_LOCK".to_string(),
        KeyCode::OPTION => "OPTION".to_string(),
        KeyCode::CONTROL => "CONTROL".to_string(),
        KeyCode::RIGHT_COMMAND => "RIGHT_COMMAND".to_string(),
        KeyCode::RIGHT_SHIFT => "RIGHT_SHIFT".to_string(),
        KeyCode::RIGHT_OPTION => "RIGHT_OPTION".to_string(),
        KeyCode::RIGHT_CONTROL => "RIGHT_CONTROL".to_string(),
        KeyCode::FUNCTION => "FUNCTION".to_string(),
        KeyCode::VOLUME_UP => "VOLUME_UP".to_string(),
        KeyCode::VOLUME_DOWN => "VOLUME_DOWN".to_string(),
        KeyCode::MUTE => "MUTE".to_string(),
        KeyCode::F1 => "F1".to_string(),
        KeyCode::F2 => "F2".to_string(),
        KeyCode::F3 => "F3".to_string(),
        KeyCode::F4 => "F4".to_string(),
        KeyCode::F5 => "F5".to_string(),
        KeyCode::F6 => "F6".to_string(),
        KeyCode::F7 => "F7".to_string(),
        KeyCode::F8 => "F8".to_string(),
        KeyCode::F9 => "F9".to_string(),
        KeyCode::F10 => "F10".to_string(),
        KeyCode::F11 => "F11".to_string(),
        KeyCode::F12 => "F12".to_string(),
        KeyCode::F13 => "F13".to_string(),
        KeyCode::F14 => "F14".to_string(),
        KeyCode::F15 => "F15".to_string(),
        KeyCode::F16 => "F16".to_string(),
        KeyCode::F17 => "F17".to_string(),
        KeyCode::F18 => "F18".to_string(),
        KeyCode::F19 => "F19".to_string(),
        KeyCode::F20 => "F20".to_string(),
        KeyCode::HELP => "HELP".to_string(),
        KeyCode::HOME => "HOME".to_string(),
        KeyCode::PAGE_UP => "PAGE_UP".to_string(),
        KeyCode::FORWARD_DELETE => "FORWARD_DELETE".to_string(),
        KeyCode::END => "END".to_string(),
        KeyCode::PAGE_DOWN => "PAGE_DOWN".to_string(),
        KeyCode::LEFT_ARROW => "LEFT_ARROW".to_string(),
        KeyCode::RIGHT_ARROW => "RIGHT_ARROW".to_string(),
        KeyCode::DOWN_ARROW => "DOWN_ARROW".to_string(),
        KeyCode::UP_ARROW => "UP_ARROW".to_string(),
        other => format!("0x{other:02X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    fn source() -> CGEventSource {
        CGEventSource::new(CGEventSourceStateID::HIDSystemState).expect("event source")
    }

    fn key_event(code: CGKeyCode, is_down: bool) -> CGEvent {
        CGEvent::new_keyboard_event(source(), code, is_down).expect("keyboard event")
    }

    #[test]
    fn plain_key_press_and_release_carry_no_modifiers() {
        let mut translator = EventTranslator::new();

        let down = translator
            .translate(CGEventType::KeyDown, &key_event(0x00, true), 0)
            .unwrap();
        assert_eq!(
            down,
            InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "0x00".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            })
        );

        let up = translator
            .translate(CGEventType::KeyUp, &key_event(0x00, false), 0)
            .unwrap();
        assert_eq!(
            up,
            InputEvent::Keyboard(KeyboardEvent::KeyUp {
                key: "0x00".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            })
        );
    }

    #[test]
    fn named_keys_use_their_kecode_constants_name() {
        let mut translator = EventTranslator::new();
        let down = translator
            .translate(CGEventType::KeyDown, &key_event(KeyCode::RETURN, true), 0)
            .unwrap();
        assert_eq!(
            down,
            InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "RETURN".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            })
        );
    }

    #[test]
    fn a_flags_changed_event_that_newly_sets_the_shift_bit_is_a_key_down() {
        let mut translator = EventTranslator::new();
        let event = key_event(KeyCode::SHIFT, true);
        event.set_flags(CGEventFlags::CGEventFlagShift);

        let down = translator
            .translate(CGEventType::FlagsChanged, &event, 0)
            .unwrap();
        assert_eq!(
            down,
            InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "SHIFT".to_string(),
                modifiers: vec![Modifier::Shift],
                timestamp_ms: 0,
            })
        );
    }

    #[test]
    fn releasing_a_modifier_clears_the_flag_and_stops_it_being_reported() {
        let mut translator = EventTranslator::new();
        let down = key_event(KeyCode::CONTROL, true);
        down.set_flags(CGEventFlags::CGEventFlagControl);
        translator
            .translate(CGEventType::FlagsChanged, &down, 0)
            .unwrap();

        let up = key_event(KeyCode::CONTROL, false);
        up.set_flags(CGEventFlags::CGEventFlagNull);
        let released = translator
            .translate(CGEventType::FlagsChanged, &up, 0)
            .unwrap();
        assert_eq!(
            released,
            InputEvent::Keyboard(KeyboardEvent::KeyUp {
                key: "CONTROL".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            })
        );
    }

    #[test]
    fn a_flags_changed_event_with_no_state_change_is_dropped() {
        let mut translator = EventTranslator::new();
        let event = key_event(KeyCode::SHIFT, true);
        event.set_flags(CGEventFlags::CGEventFlagNull);
        assert!(translator
            .translate(CGEventType::FlagsChanged, &event, 0)
            .is_none());
    }

    #[test]
    fn held_shift_is_reported_on_a_later_key() {
        let mut translator = EventTranslator::new();
        let shift_down = key_event(KeyCode::SHIFT, true);
        shift_down.set_flags(CGEventFlags::CGEventFlagShift);
        translator
            .translate(CGEventType::FlagsChanged, &shift_down, 0)
            .unwrap();

        let down = translator
            .translate(CGEventType::KeyDown, &key_event(0x00, true), 0)
            .unwrap();
        assert_eq!(
            down,
            InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "0x00".to_string(),
                modifiers: vec![Modifier::Shift],
                timestamp_ms: 0,
            })
        );
    }

    #[test]
    fn left_and_right_mouse_buttons_translate_directly() {
        let mut translator = EventTranslator::new();
        let event = CGEvent::new_mouse_event(
            source(),
            CGEventType::LeftMouseDown,
            core_graphics::geometry::CGPoint::new(0.0, 0.0),
            core_graphics::event::CGMouseButton::Left,
        )
        .expect("mouse event");
        assert_eq!(
            translator.translate(CGEventType::LeftMouseDown, &event, 0),
            Some(InputEvent::Mouse(MouseEvent::ButtonDown {
                button: MouseButton::Left,
                timestamp_ms: 0,
            }))
        );
    }

    #[test]
    fn other_mouse_button_two_is_the_middle_button() {
        let mut translator = EventTranslator::new();
        let event = CGEvent::new_mouse_event(
            source(),
            CGEventType::OtherMouseDown,
            core_graphics::geometry::CGPoint::new(0.0, 0.0),
            core_graphics::event::CGMouseButton::Center,
        )
        .expect("mouse event");
        event.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, 2);
        assert_eq!(
            translator.translate(CGEventType::OtherMouseDown, &event, 0),
            Some(InputEvent::Mouse(MouseEvent::ButtonDown {
                button: MouseButton::Middle,
                timestamp_ms: 0,
            }))
        );
    }

    #[test]
    fn other_mouse_button_beyond_middle_is_dropped() {
        let mut translator = EventTranslator::new();
        let event = CGEvent::new_mouse_event(
            source(),
            CGEventType::OtherMouseDown,
            core_graphics::geometry::CGPoint::new(0.0, 0.0),
            core_graphics::event::CGMouseButton::Center,
        )
        .expect("mouse event");
        event.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, 3);
        assert!(translator
            .translate(CGEventType::OtherMouseDown, &event, 0)
            .is_none());
    }

    #[test]
    fn mouse_moved_reads_the_delta_fields() {
        let mut translator = EventTranslator::new();
        let event = CGEvent::new_mouse_event(
            source(),
            CGEventType::MouseMoved,
            core_graphics::geometry::CGPoint::new(0.0, 0.0),
            core_graphics::event::CGMouseButton::Left,
        )
        .expect("mouse event");
        event.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_X, 5);
        event.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y, -3);
        assert_eq!(
            translator.translate(CGEventType::MouseMoved, &event, 0),
            Some(InputEvent::Mouse(MouseEvent::Move {
                dx: 5,
                dy: -3,
                timestamp_ms: 0,
            }))
        );
    }

    #[test]
    fn scroll_wheel_maps_axis_one_to_dy_and_axis_two_to_dx() {
        let mut translator = EventTranslator::new();
        let event = CGEvent::new_scroll_event(
            source(),
            core_graphics::event::ScrollEventUnit::LINE,
            2,
            2,
            -1,
            0,
        )
        .expect("scroll event");
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1, 2);
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2, -1);
        assert_eq!(
            translator.translate(CGEventType::ScrollWheel, &event, 0),
            Some(InputEvent::Mouse(MouseEvent::Scroll {
                dx: -1,
                dy: 2,
                timestamp_ms: 0,
            }))
        );
    }

    #[test]
    fn unmodeled_event_types_are_dropped() {
        let mut translator = EventTranslator::new();
        let event = key_event(0x00, true);
        assert!(translator
            .translate(CGEventType::TabletPointer, &event, 0)
            .is_none());
    }
}
