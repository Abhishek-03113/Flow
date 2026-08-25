//! Pure translation from the low-level keyboard/mouse hook structs
//! (`KBDLLHOOKSTRUCT`/`MSLLHOOKSTRUCT`) to `flow_core`'s
//! platform-independent [`InputEvent`] (vision.md §11). Isolated from any
//! hook installation so it's unit-testable without one
//! (`daemon/todos.json` E6 acceptance criteria) — though, like the rest
//! of this module, the tests here can only actually run on Windows; this
//! session verified them by cross-compiling only (`daemon/README.md`).

use std::collections::HashSet;

use flow_core::protocol::{InputEvent, KeyboardEvent, Modifier, MouseButton, MouseEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_0, VK_9, VK_A, VK_APPS, VK_BACK, VK_CAPITAL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1,
    VK_F24, VK_HOME, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_NEXT, VK_PRIOR,
    VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SPACE, VK_TAB, VK_UP, VK_Z,
};
use windows::Win32::UI::WindowsAndMessaging::{
    KBDLLHOOKSTRUCT, MSLLHOOKSTRUCT, WHEEL_DELTA, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Converts low-level hook events into `flow_core` [`InputEvent`]s.
///
/// Stateful for two reasons a single event alone can't cover: the
/// low-level mouse hook reports an *absolute* cursor position, not the
/// relative delta `MouseEvent::Move` needs, so consecutive positions have
/// to be diffed; and, as on every other platform this crate supports,
/// modifier-key state has to be tracked so every keyboard event carries
/// the full modifier list `docs/contracts/data-model.md` expects.
#[derive(Debug, Default)]
pub struct EventTranslator {
    held_modifiers: HashSet<Modifier>,
    last_mouse_position: Option<(i32, i32)>,
}

impl EventTranslator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Translates one `WH_KEYBOARD_LL` callback. `message` is the hook's
    /// `wparam` (`WM_KEYDOWN`/`WM_KEYUP`/`WM_SYSKEYDOWN`/`WM_SYSKEYUP`);
    /// `WM_SYSKEY*` (keys pressed while Alt is held) counts the same as
    /// its non-`SYS` counterpart — Flow doesn't distinguish them.
    pub fn translate_keyboard(
        &mut self,
        message: u32,
        info: &KBDLLHOOKSTRUCT,
        timestamp_ms: u64,
    ) -> Option<InputEvent> {
        let is_down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let is_up = message == WM_KEYUP || message == WM_SYSKEYUP;
        if !is_down && !is_up {
            return None;
        }

        let vk = info.vkCode as u16;
        if let Some(modifier) = modifier_for(vk) {
            if is_down {
                self.held_modifiers.insert(modifier);
            } else {
                self.held_modifiers.remove(&modifier);
            }
        }

        let key = key_name(vk);
        let modifiers = self.modifiers_snapshot();
        Some(InputEvent::Keyboard(if is_down {
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

    /// Translates one `WH_MOUSE_LL` callback. `message` is the hook's
    /// `wparam` (`WM_MOUSEMOVE`, `WM_LBUTTONDOWN`, ...).
    pub fn translate_mouse(
        &mut self,
        message: u32,
        info: &MSLLHOOKSTRUCT,
        timestamp_ms: u64,
    ) -> Option<InputEvent> {
        match message {
            m if m == WM_MOUSEMOVE => self.translate_move(info.pt.x, info.pt.y, timestamp_ms),
            m if m == WM_LBUTTONDOWN => Some(button_event(MouseButton::Left, true, timestamp_ms)),
            m if m == WM_LBUTTONUP => Some(button_event(MouseButton::Left, false, timestamp_ms)),
            m if m == WM_RBUTTONDOWN => Some(button_event(MouseButton::Right, true, timestamp_ms)),
            m if m == WM_RBUTTONUP => Some(button_event(MouseButton::Right, false, timestamp_ms)),
            m if m == WM_MBUTTONDOWN => Some(button_event(MouseButton::Middle, true, timestamp_ms)),
            m if m == WM_MBUTTONUP => Some(button_event(MouseButton::Middle, false, timestamp_ms)),
            m if m == WM_MOUSEWHEEL => Some(InputEvent::Mouse(MouseEvent::Scroll {
                dx: 0,
                dy: wheel_delta(info.mouseData),
                timestamp_ms,
            })),
            m if m == WM_MOUSEHWHEEL => Some(InputEvent::Mouse(MouseEvent::Scroll {
                dx: wheel_delta(info.mouseData),
                dy: 0,
                timestamp_ms,
            })),
            _ => None,
        }
    }

    /// The first move after (re)start has no prior position to diff
    /// against, so it's dropped rather than reported as a jump from the
    /// origin; likewise a position that hasn't actually changed (the OS
    /// can report a `WM_MOUSEMOVE` with no real movement).
    fn translate_move(&mut self, x: i32, y: i32, timestamp_ms: u64) -> Option<InputEvent> {
        let previous = self.last_mouse_position.replace((x, y));
        let (prev_x, prev_y) = previous?;
        let (dx, dy) = (x - prev_x, y - prev_y);
        if dx == 0 && dy == 0 {
            return None;
        }
        Some(InputEvent::Mouse(MouseEvent::Move {
            dx,
            dy,
            timestamp_ms,
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

fn modifier_for(vk: u16) -> Option<Modifier> {
    if vk == VK_LSHIFT.0 || vk == VK_RSHIFT.0 {
        Some(Modifier::Shift)
    } else if vk == VK_LCONTROL.0 || vk == VK_RCONTROL.0 {
        Some(Modifier::Ctrl)
    } else if vk == VK_LMENU.0 || vk == VK_RMENU.0 {
        Some(Modifier::Alt)
    } else if vk == VK_LWIN.0 || vk == VK_RWIN.0 {
        Some(Modifier::Meta)
    } else {
        None
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

/// `mouseData`'s high-order word carries the wheel delta as a signed
/// multiple of `WHEEL_DELTA` (120, one notch); this normalizes it down
/// to a small per-notch integer, matching the single-unit-per-notch
/// granularity Linux (`REL_WHEEL`) and macOS (`ScrollEventUnit::LINE`)
/// report.
fn wheel_delta(mouse_data: u32) -> i32 {
    let high_word = (mouse_data >> 16) as u16 as i16;
    i32::from(high_word) / WHEEL_DELTA as i32
}

/// Names a virtual-key code the same way `flow-platform`'s other
/// platforms name their own raw codes: a short, human-readable token.
/// Letters and digits are Windows' own virtual-key values (`VK_A`..`VK_Z`
/// are ASCII 'A'..'Z', `VK_0`..`VK_9` are ASCII '0'..'9'), and `VK_F1`..
/// `VK_F24` are contiguous, so both derive their name arithmetically
/// rather than needing a lookup; everything else this function itself
/// names falls back to a hex literal, same as the macOS side.
fn key_name(vk: u16) -> String {
    if (VK_0.0..=VK_9.0).contains(&vk) || (VK_A.0..=VK_Z.0).contains(&vk) {
        return (vk as u8 as char).to_string();
    }
    if (VK_F1.0..=VK_F24.0).contains(&vk) {
        return format!("F{}", vk - VK_F1.0 + 1);
    }
    let name = if vk == VK_RETURN.0 {
        "RETURN"
    } else if vk == VK_ESCAPE.0 {
        "ESCAPE"
    } else if vk == VK_SPACE.0 {
        "SPACE"
    } else if vk == VK_TAB.0 {
        "TAB"
    } else if vk == VK_BACK.0 {
        "BACK"
    } else if vk == VK_DELETE.0 {
        "DELETE"
    } else if vk == VK_CAPITAL.0 {
        "CAPITAL"
    } else if vk == VK_HOME.0 {
        "HOME"
    } else if vk == VK_END.0 {
        "END"
    } else if vk == VK_PRIOR.0 {
        "PRIOR"
    } else if vk == VK_NEXT.0 {
        "NEXT"
    } else if vk == VK_LEFT.0 {
        "LEFT"
    } else if vk == VK_RIGHT.0 {
        "RIGHT"
    } else if vk == VK_UP.0 {
        "UP"
    } else if vk == VK_DOWN.0 {
        "DOWN"
    } else if vk == VK_APPS.0 {
        "APPS"
    } else if vk == VK_LSHIFT.0 {
        "LSHIFT"
    } else if vk == VK_RSHIFT.0 {
        "RSHIFT"
    } else if vk == VK_LCONTROL.0 {
        "LCONTROL"
    } else if vk == VK_RCONTROL.0 {
        "RCONTROL"
    } else if vk == VK_LMENU.0 {
        "LMENU"
    } else if vk == VK_RMENU.0 {
        "RMENU"
    } else if vk == VK_LWIN.0 {
        "LWIN"
    } else if vk == VK_RWIN.0 {
        "RWIN"
    } else {
        return format!("0x{vk:02X}");
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_5;

    fn key_info(vk: u16) -> KBDLLHOOKSTRUCT {
        KBDLLHOOKSTRUCT {
            vkCode: u32::from(vk),
            ..Default::default()
        }
    }

    fn mouse_info(x: i32, y: i32, mouse_data: u32) -> MSLLHOOKSTRUCT {
        MSLLHOOKSTRUCT {
            pt: POINT { x, y },
            mouseData: mouse_data,
            ..Default::default()
        }
    }

    #[test]
    fn plain_key_press_and_release_carry_no_modifiers() {
        let mut translator = EventTranslator::new();

        let down = translator
            .translate_keyboard(WM_KEYDOWN, &key_info(VK_A.0), 0)
            .unwrap();
        assert_eq!(
            down,
            InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "A".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            })
        );

        let up = translator
            .translate_keyboard(WM_KEYUP, &key_info(VK_A.0), 0)
            .unwrap();
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
    fn a_syskeydown_counts_the_same_as_a_plain_keydown() {
        let mut translator = EventTranslator::new();
        let down = translator
            .translate_keyboard(WM_SYSKEYDOWN, &key_info(VK_F1.0), 0)
            .unwrap();
        assert_eq!(
            down,
            InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "F1".to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            })
        );
    }

    #[test]
    fn held_shift_is_reported_on_a_later_key() {
        let mut translator = EventTranslator::new();
        translator
            .translate_keyboard(WM_KEYDOWN, &key_info(VK_LSHIFT.0), 0)
            .unwrap();

        let down = translator
            .translate_keyboard(WM_KEYDOWN, &key_info(VK_A.0), 0)
            .unwrap();
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
            .translate_keyboard(WM_KEYDOWN, &key_info(VK_LCONTROL.0), 0)
            .unwrap();
        translator
            .translate_keyboard(WM_KEYUP, &key_info(VK_LCONTROL.0), 0)
            .unwrap();

        let down = translator
            .translate_keyboard(WM_KEYDOWN, &key_info(VK_A.0), 0)
            .unwrap();
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
    fn digits_and_letters_name_themselves() {
        assert_eq!(key_name(VK_5.0), "5".to_string());
        assert_eq!(key_name(VK_A.0), "A".to_string());
    }

    #[test]
    fn a_first_mouse_move_has_no_prior_position_and_is_dropped() {
        let mut translator = EventTranslator::new();
        assert!(translator
            .translate_mouse(WM_MOUSEMOVE, &mouse_info(10, 10, 0), 0)
            .is_none());
    }

    #[test]
    fn a_second_mouse_move_reports_the_delta_from_the_first() {
        let mut translator = EventTranslator::new();
        // First move is intentionally dropped (see above) — it only
        // establishes the starting position.
        assert!(translator
            .translate_mouse(WM_MOUSEMOVE, &mouse_info(10, 10, 0), 0)
            .is_none());

        let moved = translator
            .translate_mouse(WM_MOUSEMOVE, &mouse_info(15, 7, 0), 0)
            .unwrap();
        assert_eq!(
            moved,
            InputEvent::Mouse(MouseEvent::Move {
                dx: 5,
                dy: -3,
                timestamp_ms: 0,
            })
        );
    }

    #[test]
    fn mouse_buttons_translate_to_button_events() {
        let mut translator = EventTranslator::new();
        assert_eq!(
            translator.translate_mouse(WM_LBUTTONDOWN, &mouse_info(0, 0, 0), 0),
            Some(InputEvent::Mouse(MouseEvent::ButtonDown {
                button: MouseButton::Left,
                timestamp_ms: 0,
            }))
        );
        assert_eq!(
            translator.translate_mouse(WM_RBUTTONUP, &mouse_info(0, 0, 0), 0),
            Some(InputEvent::Mouse(MouseEvent::ButtonUp {
                button: MouseButton::Right,
                timestamp_ms: 0,
            }))
        );
    }

    #[test]
    fn wheel_events_normalize_to_one_unit_per_notch() {
        let mut translator = EventTranslator::new();
        let one_notch_up: u32 = WHEEL_DELTA << 16;
        assert_eq!(
            translator.translate_mouse(WM_MOUSEWHEEL, &mouse_info(0, 0, one_notch_up), 0),
            Some(InputEvent::Mouse(MouseEvent::Scroll {
                dx: 0,
                dy: 1,
                timestamp_ms: 0,
            }))
        );

        let one_notch_left: u32 = ((-(WHEEL_DELTA as i32)) as u16 as u32) << 16;
        assert_eq!(
            translator.translate_mouse(WM_MOUSEHWHEEL, &mouse_info(0, 0, one_notch_left), 0),
            Some(InputEvent::Mouse(MouseEvent::Scroll {
                dx: -1,
                dy: 0,
                timestamp_ms: 0,
            }))
        );
    }
}
