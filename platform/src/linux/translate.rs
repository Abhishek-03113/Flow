//! Pure translation from evdev's raw event model to `flow_core`'s
//! platform-independent [`InputEvent`] (vision.md §11). Isolated from any
//! device I/O so it's unit-testable without hardware access
//! (`daemon/todos.json` E1 acceptance criteria).

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use evdev::{EventSummary, KeyCode, RelativeAxisCode};
use flow_core::protocol::{key_names, InputEvent, KeyboardEvent, Modifier, MouseButton, MouseEvent};

/// evdev's `EV_KEY` value for a press. Shared with `inject_translate`,
/// which needs the same values going the other direction.
pub(super) const KEY_DOWN: i32 = 1;
/// evdev's `EV_KEY` value for a release. Autorepeat (value `2`) has no
/// equivalent in the contract and is dropped.
pub(super) const KEY_UP: i32 = 0;

/// Converts raw evdev events into `flow_core` [`InputEvent`]s.
///
/// Stateful only to track which modifier keys are currently held, so every
/// keyboard event it emits carries the full modifier set
/// `docs/contracts/data-model.md` expects, matching what a single evdev key
/// event alone can't tell you.
#[derive(Debug, Default)]
pub struct EventTranslator {
    held_modifiers: HashSet<Modifier>,
}

impl EventTranslator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Translates one evdev event. Returns `None` for event kinds Flow
    /// doesn't model (`SYN_REPORT`, absolute axes, LEDs, ...) and for key
    /// autorepeat, which the contract has no representation for.
    pub fn translate(&mut self, event: evdev::InputEvent) -> Option<InputEvent> {
        let timestamp_ms = timestamp_ms(event.timestamp());
        match event.destructure() {
            EventSummary::Key(_, code, value) => self.translate_key(code, value, timestamp_ms),
            EventSummary::RelativeAxis(_, code, value) => {
                translate_relative_axis(code, value, timestamp_ms)
            }
            _ => None,
        }
    }

    fn translate_key(
        &mut self,
        code: KeyCode,
        value: i32,
        timestamp_ms: u64,
    ) -> Option<InputEvent> {
        if let Some(button) = mouse_button_for(code) {
            return translate_mouse_button(button, value, timestamp_ms);
        }
        if value != KEY_DOWN && value != KEY_UP {
            return None;
        }
        if let Some(modifier) = modifier_for(code) {
            if value == KEY_DOWN {
                self.held_modifiers.insert(modifier);
            } else {
                self.held_modifiers.remove(&modifier);
            }
        }
        let key = key_name(code);
        let modifiers = self.modifiers_snapshot();
        Some(InputEvent::Keyboard(if value == KEY_DOWN {
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

fn modifier_for(code: KeyCode) -> Option<Modifier> {
    match code {
        KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT => Some(Modifier::Shift),
        KeyCode::KEY_LEFTCTRL | KeyCode::KEY_RIGHTCTRL => Some(Modifier::Ctrl),
        KeyCode::KEY_LEFTALT | KeyCode::KEY_RIGHTALT => Some(Modifier::Alt),
        KeyCode::KEY_LEFTMETA | KeyCode::KEY_RIGHTMETA => Some(Modifier::Meta),
        _ => None,
    }
}

fn mouse_button_for(code: KeyCode) -> Option<MouseButton> {
    match code {
        KeyCode::BTN_LEFT => Some(MouseButton::Left),
        KeyCode::BTN_RIGHT => Some(MouseButton::Right),
        KeyCode::BTN_MIDDLE => Some(MouseButton::Middle),
        _ => None,
    }
}

fn translate_mouse_button(
    button: MouseButton,
    value: i32,
    timestamp_ms: u64,
) -> Option<InputEvent> {
    match value {
        KEY_DOWN => Some(InputEvent::Mouse(MouseEvent::ButtonDown {
            button,
            timestamp_ms,
        })),
        KEY_UP => Some(InputEvent::Mouse(MouseEvent::ButtonUp {
            button,
            timestamp_ms,
        })),
        _ => None,
    }
}

/// evdev reports each relative axis as its own event ahead of a
/// `SYN_REPORT`, rather than a combined `(dx, dy)` sample — so a physical
/// mouse move typically becomes two [`MouseEvent::Move`]s in quick
/// succession (one per axis) rather than one. Deliberate simplification:
/// documented in `daemon/todos.json` E1's `buildNote` rather than hidden.
fn translate_relative_axis(
    code: RelativeAxisCode,
    value: i32,
    timestamp_ms: u64,
) -> Option<InputEvent> {
    match code {
        RelativeAxisCode::REL_X => Some(InputEvent::Mouse(MouseEvent::Move {
            dx: value,
            dy: 0,
            timestamp_ms,
        })),
        RelativeAxisCode::REL_Y => Some(InputEvent::Mouse(MouseEvent::Move {
            dx: 0,
            dy: value,
            timestamp_ms,
        })),
        RelativeAxisCode::REL_WHEEL => Some(InputEvent::Mouse(MouseEvent::Scroll {
            dx: 0,
            dy: value,
            timestamp_ms,
        })),
        RelativeAxisCode::REL_HWHEEL => Some(InputEvent::Mouse(MouseEvent::Scroll {
            dx: value,
            dy: 0,
            timestamp_ms,
        })),
        _ => None,
    }
}

/// Maps an evdev code that has a cross-platform equivalent to the shared
/// `key_names` vocabulary. Letters, digits, and evdev-only keys with no
/// equivalent on Windows/macOS (e.g. `KEY_LEFTBRACE`'s neighbors that
/// don't exist there) fall through to the generic strip-`KEY_`-prefix
/// passthrough below, which already matches the bare-char convention for
/// plain letters/digits.
fn key_name(code: KeyCode) -> String {
    match code {
        KeyCode::KEY_ENTER => key_names::RETURN.to_string(),
        KeyCode::KEY_TAB => key_names::TAB.to_string(),
        KeyCode::KEY_SPACE => key_names::SPACE.to_string(),
        KeyCode::KEY_BACKSPACE => key_names::DELETE.to_string(),
        KeyCode::KEY_DELETE => key_names::FORWARD_DELETE.to_string(),
        KeyCode::KEY_ESC => key_names::ESCAPE.to_string(),
        KeyCode::KEY_LEFTSHIFT => key_names::SHIFT.to_string(),
        KeyCode::KEY_RIGHTSHIFT => key_names::RIGHT_SHIFT.to_string(),
        KeyCode::KEY_LEFTCTRL => key_names::CONTROL.to_string(),
        KeyCode::KEY_RIGHTCTRL => key_names::RIGHT_CONTROL.to_string(),
        KeyCode::KEY_LEFTALT => key_names::OPTION.to_string(),
        KeyCode::KEY_RIGHTALT => key_names::RIGHT_OPTION.to_string(),
        KeyCode::KEY_LEFTMETA => key_names::COMMAND.to_string(),
        KeyCode::KEY_RIGHTMETA => key_names::RIGHT_COMMAND.to_string(),
        KeyCode::KEY_CAPSLOCK => key_names::CAPS_LOCK.to_string(),
        KeyCode::KEY_FN => key_names::FUNCTION.to_string(),
        KeyCode::KEY_HOME => key_names::HOME.to_string(),
        KeyCode::KEY_END => key_names::END.to_string(),
        KeyCode::KEY_PAGEUP => key_names::PAGE_UP.to_string(),
        KeyCode::KEY_PAGEDOWN => key_names::PAGE_DOWN.to_string(),
        KeyCode::KEY_LEFT => key_names::LEFT_ARROW.to_string(),
        KeyCode::KEY_RIGHT => key_names::RIGHT_ARROW.to_string(),
        KeyCode::KEY_UP => key_names::UP_ARROW.to_string(),
        KeyCode::KEY_DOWN => key_names::DOWN_ARROW.to_string(),
        KeyCode::KEY_HELP => key_names::HELP.to_string(),
        KeyCode::KEY_VOLUMEUP => key_names::VOLUME_UP.to_string(),
        KeyCode::KEY_VOLUMEDOWN => key_names::VOLUME_DOWN.to_string(),
        KeyCode::KEY_MUTE => key_names::MUTE.to_string(),
        KeyCode::KEY_MINUS => key_names::MINUS.to_string(),
        KeyCode::KEY_EQUAL => key_names::EQUAL.to_string(),
        KeyCode::KEY_LEFTBRACE => key_names::LEFT_BRACKET.to_string(),
        KeyCode::KEY_RIGHTBRACE => key_names::RIGHT_BRACKET.to_string(),
        KeyCode::KEY_SEMICOLON => key_names::SEMICOLON.to_string(),
        KeyCode::KEY_APOSTROPHE => key_names::QUOTE.to_string(),
        KeyCode::KEY_COMMA => key_names::COMMA.to_string(),
        KeyCode::KEY_DOT => key_names::PERIOD.to_string(),
        KeyCode::KEY_SLASH => key_names::SLASH.to_string(),
        KeyCode::KEY_BACKSLASH => key_names::BACKSLASH.to_string(),
        KeyCode::KEY_GRAVE => key_names::GRAVE.to_string(),
        // evdev's F-keys aren't numbered contiguously: F1-F10 sit in one
        // range, F11-F12 in another, F13-F24 in a third — so these are
        // enumerated individually rather than derived arithmetically.
        KeyCode::KEY_F1 => key_names::function_key(1),
        KeyCode::KEY_F2 => key_names::function_key(2),
        KeyCode::KEY_F3 => key_names::function_key(3),
        KeyCode::KEY_F4 => key_names::function_key(4),
        KeyCode::KEY_F5 => key_names::function_key(5),
        KeyCode::KEY_F6 => key_names::function_key(6),
        KeyCode::KEY_F7 => key_names::function_key(7),
        KeyCode::KEY_F8 => key_names::function_key(8),
        KeyCode::KEY_F9 => key_names::function_key(9),
        KeyCode::KEY_F10 => key_names::function_key(10),
        KeyCode::KEY_F11 => key_names::function_key(11),
        KeyCode::KEY_F12 => key_names::function_key(12),
        KeyCode::KEY_F13 => key_names::function_key(13),
        KeyCode::KEY_F14 => key_names::function_key(14),
        KeyCode::KEY_F15 => key_names::function_key(15),
        KeyCode::KEY_F16 => key_names::function_key(16),
        KeyCode::KEY_F17 => key_names::function_key(17),
        KeyCode::KEY_F18 => key_names::function_key(18),
        KeyCode::KEY_F19 => key_names::function_key(19),
        KeyCode::KEY_F20 => key_names::function_key(20),
        other => {
            let debug = format!("{other:?}");
            debug.strip_prefix("KEY_").unwrap_or(&debug).to_owned()
        }
    }
}

fn timestamp_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use evdev::EventType;

    fn key_event(code: KeyCode, value: i32) -> evdev::InputEvent {
        evdev::InputEvent::new(EventType::KEY.0, code.0, value)
    }

    fn rel_event(code: RelativeAxisCode, value: i32) -> evdev::InputEvent {
        evdev::InputEvent::new(EventType::RELATIVE.0, code.0, value)
    }

    #[test]
    fn plain_key_press_and_release_carry_no_modifiers() {
        let mut translator = EventTranslator::new();

        let down = translator.translate(key_event(KeyCode::KEY_A, 1)).unwrap();
        assert_eq!(
            down,
            InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "A".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            })
        );

        let up = translator.translate(key_event(KeyCode::KEY_A, 0)).unwrap();
        assert_eq!(
            up,
            InputEvent::Keyboard(KeyboardEvent::KeyUp {
                key: "A".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            })
        );
    }

    #[test]
    fn autorepeat_is_dropped() {
        let mut translator = EventTranslator::new();
        assert!(translator.translate(key_event(KeyCode::KEY_A, 2)).is_none());
    }

    #[test]
    fn held_shift_is_reported_on_a_later_key() {
        let mut translator = EventTranslator::new();
        translator
            .translate(key_event(KeyCode::KEY_LEFTSHIFT, 1))
            .unwrap();

        let down = translator.translate(key_event(KeyCode::KEY_A, 1)).unwrap();
        assert_eq!(
            down,
            InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "A".to_string(),
                modifiers: vec![Modifier::Shift],
                timestamp_ms: 0,
            })
        );
    }

    #[test]
    fn releasing_a_modifier_stops_it_being_reported() {
        let mut translator = EventTranslator::new();
        translator
            .translate(key_event(KeyCode::KEY_LEFTCTRL, 1))
            .unwrap();
        translator
            .translate(key_event(KeyCode::KEY_LEFTCTRL, 0))
            .unwrap();

        let down = translator.translate(key_event(KeyCode::KEY_A, 1)).unwrap();
        assert_eq!(
            down,
            InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "A".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            })
        );
    }

    #[test]
    fn modifiers_snapshot_is_in_a_fixed_order_regardless_of_press_order() {
        let mut translator = EventTranslator::new();
        translator
            .translate(key_event(KeyCode::KEY_LEFTMETA, 1))
            .unwrap();
        translator
            .translate(key_event(KeyCode::KEY_LEFTCTRL, 1))
            .unwrap();
        translator
            .translate(key_event(KeyCode::KEY_LEFTSHIFT, 1))
            .unwrap();

        let down = translator.translate(key_event(KeyCode::KEY_A, 1)).unwrap();
        assert_eq!(
            down,
            InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "A".to_string(),
                modifiers: vec![Modifier::Shift, Modifier::Ctrl, Modifier::Meta],
                timestamp_ms: 0,
            })
        );
    }

    #[test]
    fn mouse_buttons_translate_to_button_events_not_keyboard_events() {
        let mut translator = EventTranslator::new();
        let down = translator
            .translate(key_event(KeyCode::BTN_LEFT, 1))
            .unwrap();
        assert_eq!(
            down,
            InputEvent::Mouse(MouseEvent::ButtonDown {
                button: MouseButton::Left,
                timestamp_ms: 0,
            })
        );

        let up = translator
            .translate(key_event(KeyCode::BTN_RIGHT, 0))
            .unwrap();
        assert_eq!(
            up,
            InputEvent::Mouse(MouseEvent::ButtonUp {
                button: MouseButton::Right,
                timestamp_ms: 0,
            })
        );
    }

    #[test]
    fn relative_axes_translate_to_move_and_scroll() {
        let mut translator = EventTranslator::new();
        assert_eq!(
            translator.translate(rel_event(RelativeAxisCode::REL_X, 5)),
            Some(InputEvent::Mouse(MouseEvent::Move {
                dx: 5,
                dy: 0,
                timestamp_ms: 0,
            }))
        );
        assert_eq!(
            translator.translate(rel_event(RelativeAxisCode::REL_Y, -3)),
            Some(InputEvent::Mouse(MouseEvent::Move {
                dx: 0,
                dy: -3,
                timestamp_ms: 0,
            }))
        );
        assert_eq!(
            translator.translate(rel_event(RelativeAxisCode::REL_WHEEL, 1)),
            Some(InputEvent::Mouse(MouseEvent::Scroll {
                dx: 0,
                dy: 1,
                timestamp_ms: 0,
            }))
        );
        assert_eq!(
            translator.translate(rel_event(RelativeAxisCode::REL_HWHEEL, -1)),
            Some(InputEvent::Mouse(MouseEvent::Scroll {
                dx: -1,
                dy: 0,
                timestamp_ms: 0,
            }))
        );
    }

    #[test]
    fn unmapped_relative_axes_are_dropped() {
        let mut translator = EventTranslator::new();
        assert!(translator
            .translate(rel_event(RelativeAxisCode::REL_Z, 1))
            .is_none());
    }
}
