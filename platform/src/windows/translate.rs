//! Pure translation from the low-level keyboard/mouse hook structs
//! (`KBDLLHOOKSTRUCT`/`MSLLHOOKSTRUCT`) to `flow_core`'s
//! platform-independent [`InputEvent`] (vision.md §11). Isolated from any
//! hook installation so it's unit-testable without one
//! (`docs/tasks/daemon-todos.json` E6 acceptance criteria) — though, like the rest
//! of this module, the tests here can only actually run on Windows; this
//! session verified them by cross-compiling only (`daemon/README.md`).

use std::collections::HashSet;

use flow_core::protocol::{InputEvent, KeyboardEvent, Modifier, MouseButton, MouseEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
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

    /// Overrides the position the *next* `translate_move` diffs against,
    /// without emitting an event. [`super::capture`]'s local-suppression
    /// path calls this right after warping the real cursor back to a
    /// fixed anchor point with `SetCursorPos` — see its call site for why:
    /// `MSLLHOOKSTRUCT.pt` stops reflecting genuine relative motion once
    /// this machine's own hook starts withholding `WM_MOUSEMOVE`, so the
    /// anchor point, not wherever `pt` last raw-reported, has to be what
    /// the next real move's delta is computed from.
    pub fn reset_move_anchor(&mut self, x: i32, y: i32) {
        self.last_mouse_position = Some((x, y));
    }

    /// The first move after (re)start has no prior position to diff
    /// against, so it's dropped rather than reported as a jump from the
    /// origin; likewise a position that hasn't actually changed (the OS
    /// can report a `WM_MOUSEMOVE` with no real movement).
    fn translate_move(&mut self, x: i32, y: i32, timestamp_ms: u64) -> Option<InputEvent> {
        let previous = self.last_mouse_position.replace((x, y));
        let (prev_x, prev_y) = previous?;
        // `saturating_sub`, never plain `-`: this runs inside an
        // `extern "system"` hook callback that cannot unwind, so a
        // debug-build overflow panic here aborts the daemon. A delta
        // that overflows `i32` (a jump across the full virtual-desktop
        // coordinate range, an injected absolute position) is not a
        // meaningful mouse motion anyway — clamp it.
        let (dx, dy) = (x.saturating_sub(prev_x), y.saturating_sub(prev_y));
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

/// Names a virtual-key code via the host keymap
/// (`config/keymaps/windows.toml`); unlisted codes become a `0xNN` hex
/// literal, which `inject_translate` accepts back.
fn key_name(vk: u16) -> String {
    crate::keymap::host()
        .name_for_code(u32::from(vk))
        .map_or_else(|| format!("0x{vk:02X}"), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow_core::protocol::key_names;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_5, VK_A, VK_F1};

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

    /// `capture::mouse_proc`'s recentering fix depends on this: once the
    /// real cursor has been warped back to a fixed anchor (because the
    /// prior move was withheld from local delivery), the *next* move must
    /// diff against that anchor — not against wherever the just-withheld
    /// event's raw, now-stale `pt` was — or the very next reported delta
    /// would be a bogus jump of roughly `anchor - stale_pt` instead of the
    /// real motion since the warp.
    #[test]
    fn reset_move_anchor_makes_the_next_move_diff_from_the_anchor_not_the_last_raw_position() {
        let mut translator = EventTranslator::new();
        assert!(translator
            .translate_mouse(WM_MOUSEMOVE, &mouse_info(10, 10, 0), 0)
            .is_none());
        // This move's raw position (1000, 1000) is far from where the
        // cursor was actually warped back to.
        translator
            .translate_mouse(WM_MOUSEMOVE, &mouse_info(1000, 1000, 0), 0)
            .unwrap();
        translator.reset_move_anchor(500, 500);

        let moved = translator
            .translate_mouse(WM_MOUSEMOVE, &mouse_info(506, 495, 0), 0)
            .unwrap();
        assert_eq!(
            moved,
            InputEvent::Mouse(MouseEvent::Move {
                dx: 6,
                dy: -5,
                timestamp_ms: 0,
            })
        );
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

    /// `translate_mouse` runs inside `mouse_proc`, an `extern "system"`
    /// hook callback that CANNOT unwind — a panic there aborts the whole
    /// daemon. So the absolute-position diff must never overflow-panic,
    /// no matter what `MSLLHOOKSTRUCT.pt` holds (a far virtual-desktop
    /// coordinate, an injected absolute position, garbage). A delta that
    /// genuinely can't fit in `i32` is meaningless anyway; saturating it
    /// is the sane clamp.
    #[test]
    fn a_mouse_move_across_the_full_i32_range_saturates_instead_of_panicking() {
        let mut translator = EventTranslator::new();
        // First move only records the starting position.
        assert!(translator
            .translate_mouse(WM_MOUSEMOVE, &mouse_info(i32::MIN, i32::MIN, 0), 0)
            .is_none());
        // A jump to the opposite extreme: the true delta is
        // `i32::MAX - i32::MIN`, which does not fit in an `i32`.
        let moved = translator
            .translate_mouse(WM_MOUSEMOVE, &mouse_info(i32::MAX, i32::MAX, 0), 0)
            .expect("still yields a Move, clamped");
        assert_eq!(
            moved,
            InputEvent::Mouse(MouseEvent::Move {
                dx: i32::MAX,
                dy: i32::MAX,
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

    #[test]
    fn the_embedded_keymap_agrees_with_win32_virtual_key_constants() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            VK_BACK, VK_LWIN, VK_OEM_PLUS, VK_PRIOR, VK_RMENU,
        };
        assert_eq!(key_name(VK_BACK.0), key_names::DELETE);
        assert_eq!(key_name(VK_LWIN.0), key_names::COMMAND);
        assert_eq!(key_name(VK_RMENU.0), key_names::RIGHT_OPTION);
        assert_eq!(key_name(VK_OEM_PLUS.0), key_names::EQUAL);
        assert_eq!(key_name(VK_PRIOR.0), key_names::PAGE_UP);
    }
}
