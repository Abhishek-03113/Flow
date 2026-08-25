//! Pure translation from evdev's raw event model to `flow_core`'s
//! platform-independent [`InputEvent`] (vision.md §11). Isolated from any
//! device I/O so it's unit-testable without hardware access
//! (`daemon/todos.json` E1 acceptance criteria).

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use evdev::{EventSummary, KeyCode, RelativeAxisCode};
use flow_core::protocol::{InputEvent, KeyboardEvent, Modifier, MouseButton, MouseEvent};

/// evdev's `EV_KEY` value for a press.
const KEY_DOWN: i32 = 1;
/// evdev's `EV_KEY` value for a release. Autorepeat (value `2`) has no
/// equivalent in the contract and is dropped.
const KEY_UP: i32 = 0;

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

fn key_name(code: KeyCode) -> String {
    let debug = format!("{code:?}");
    debug.strip_prefix("KEY_").unwrap_or(&debug).to_owned()
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
