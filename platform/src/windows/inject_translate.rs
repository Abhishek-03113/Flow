//! Pure translation from `flow_core`'s [`InputEvent`] back to Win32
//! `INPUT` structs — the inverse of `translate::EventTranslator`, used
//! by [`super::injector::WindowsInputInjector`]. Isolated from
//! `SendInput` itself so it's unit-testable without one
//! (`daemon/todos.json` E7 acceptance criteria) — though, like the rest
//! of this module, the tests here can only actually run on Windows.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT,
    MOUSE_EVENT_FLAGS, VIRTUAL_KEY, VK_APPS, VK_BACK, VK_CAPITAL, VK_DELETE, VK_DOWN, VK_END,
    VK_ESCAPE, VK_F1, VK_HOME, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_NEXT,
    VK_PRIOR, VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SPACE, VK_TAB,
    VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::WHEEL_DELTA;

use flow_core::protocol::{InputEvent, KeyboardEvent, MouseButton, MouseEvent};

/// Translates one `InputEvent` into the `INPUT` structs that reproduce
/// it — usually one, but a [`MouseEvent::Scroll`] with both axes set
/// becomes two (`MOUSEEVENTF_WHEEL`/`HWHEEL` are mutually exclusive on a
/// single `INPUT`), the same per-axis-event shape E2's uinput injector
/// uses for Linux. Returns `None` for an unrecognized key name — there's
/// nothing to send.
pub fn to_input(event: &InputEvent) -> Option<Vec<INPUT>> {
    match event {
        InputEvent::Keyboard(keyboard_event) => {
            keyboard_to_input(keyboard_event).map(|input| vec![input])
        }
        InputEvent::Mouse(mouse_event) => Some(mouse_to_input(mouse_event)),
    }
}

fn keyboard_to_input(event: &KeyboardEvent) -> Option<INPUT> {
    let (key, is_down) = match event {
        KeyboardEvent::KeyDown { key, .. } => (key, true),
        KeyboardEvent::KeyUp { key, .. } => (key, false),
    };
    let virtual_key = code_for_name(key)?;
    let keybd_input = KEYBDINPUT {
        wVk: virtual_key,
        dwFlags: if is_down {
            Default::default()
        } else {
            KEYEVENTF_KEYUP
        },
        ..Default::default()
    };
    Some(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: keybd_input },
    })
}

fn mouse_to_input(event: &MouseEvent) -> Vec<INPUT> {
    match event {
        MouseEvent::Move { dx, dy, .. } => vec![mouse_input(MOUSEINPUT {
            dx: *dx,
            dy: *dy,
            dwFlags: MOUSEEVENTF_MOVE,
            ..Default::default()
        })],
        MouseEvent::Scroll { dx, dy, .. } => {
            let mut inputs = Vec::new();
            if *dy != 0 {
                inputs.push(mouse_input(MOUSEINPUT {
                    mouseData: wheel_mouse_data(*dy),
                    dwFlags: MOUSEEVENTF_WHEEL,
                    ..Default::default()
                }));
            }
            if *dx != 0 {
                inputs.push(mouse_input(MOUSEINPUT {
                    mouseData: wheel_mouse_data(*dx),
                    dwFlags: MOUSEEVENTF_HWHEEL,
                    ..Default::default()
                }));
            }
            inputs
        }
        MouseEvent::ButtonDown { button, .. } => vec![mouse_input(MOUSEINPUT {
            dwFlags: button_down_flags(*button),
            ..Default::default()
        })],
        MouseEvent::ButtonUp { button, .. } => vec![mouse_input(MOUSEINPUT {
            dwFlags: button_up_flags(*button),
            ..Default::default()
        })],
    }
}

fn mouse_input(mi: MOUSEINPUT) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi },
    }
}

fn button_down_flags(button: MouseButton) -> MOUSE_EVENT_FLAGS {
    match button {
        MouseButton::Left => MOUSEEVENTF_LEFTDOWN,
        MouseButton::Right => MOUSEEVENTF_RIGHTDOWN,
        MouseButton::Middle => MOUSEEVENTF_MIDDLEDOWN,
    }
}

fn button_up_flags(button: MouseButton) -> MOUSE_EVENT_FLAGS {
    match button {
        MouseButton::Left => MOUSEEVENTF_LEFTUP,
        MouseButton::Right => MOUSEEVENTF_RIGHTUP,
        MouseButton::Middle => MOUSEEVENTF_MIDDLEUP,
    }
}

/// Reverses `translate::wheel_delta`'s normalization back to the raw,
/// signed-multiple-of-`WHEEL_DELTA` form `MOUSEEVENTF_WHEEL`/`HWHEEL`
/// expect in `mouseData`.
fn wheel_mouse_data(notches: i32) -> u32 {
    (notches * WHEEL_DELTA as i32) as u32
}

/// Reverses `translate::key_name`. Letters/digits and `F1`..`F24` derive
/// their code arithmetically, the same way `key_name` derived their
/// names; everything else is a reversed lookup, falling back to parsing
/// a `"0x.."` hex literal. Any name this crate itself produced round-trips.
fn code_for_name(name: &str) -> Option<VIRTUAL_KEY> {
    if let Some(code) = single_char_code(name) {
        return Some(code);
    }
    if let Some(digits) = name.strip_prefix('F') {
        if let Ok(number) = digits.parse::<u16>() {
            if (1..=24).contains(&number) {
                return Some(VIRTUAL_KEY(VK_F1.0 + number - 1));
            }
        }
    }
    let code = if name == "RETURN" {
        VK_RETURN
    } else if name == "ESCAPE" {
        VK_ESCAPE
    } else if name == "SPACE" {
        VK_SPACE
    } else if name == "TAB" {
        VK_TAB
    } else if name == "BACK" {
        VK_BACK
    } else if name == "DELETE" {
        VK_DELETE
    } else if name == "CAPITAL" {
        VK_CAPITAL
    } else if name == "HOME" {
        VK_HOME
    } else if name == "END" {
        VK_END
    } else if name == "PRIOR" {
        VK_PRIOR
    } else if name == "NEXT" {
        VK_NEXT
    } else if name == "LEFT" {
        VK_LEFT
    } else if name == "RIGHT" {
        VK_RIGHT
    } else if name == "UP" {
        VK_UP
    } else if name == "DOWN" {
        VK_DOWN
    } else if name == "APPS" {
        VK_APPS
    } else if name == "LSHIFT" {
        VK_LSHIFT
    } else if name == "RSHIFT" {
        VK_RSHIFT
    } else if name == "LCONTROL" {
        VK_LCONTROL
    } else if name == "RCONTROL" {
        VK_RCONTROL
    } else if name == "LMENU" {
        VK_LMENU
    } else if name == "RMENU" {
        VK_RMENU
    } else if name == "LWIN" {
        VK_LWIN
    } else if name == "RWIN" {
        VK_RWIN
    } else {
        return name
            .strip_prefix("0x")
            .and_then(|digits| u16::from_str_radix(digits, 16).ok())
            .map(VIRTUAL_KEY);
    };
    Some(code)
}

fn single_char_code(name: &str) -> Option<VIRTUAL_KEY> {
    let mut chars = name.chars();
    let only_char = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    (only_char.is_ascii_uppercase() || only_char.is_ascii_digit())
        .then_some(VIRTUAL_KEY(only_char as u16))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_A;

    fn as_keybd(input: &INPUT) -> KEYBDINPUT {
        // SAFETY: test-only read of the union field this test itself
        // just wrote via `to_input`/`keyboard_to_input`.
        unsafe { input.Anonymous.ki }
    }

    fn as_mouse(input: &INPUT) -> MOUSEINPUT {
        // SAFETY: test-only read of the union field this test itself
        // just wrote via `to_input`/`mouse_to_input`.
        unsafe { input.Anonymous.mi }
    }

    #[test]
    fn a_named_key_round_trips_through_the_capture_sides_name() {
        let inputs = to_input(&InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: "A".to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].r#type, INPUT_KEYBOARD);
        assert_eq!(as_keybd(&inputs[0]).wVk, VK_A);
        assert_eq!(as_keybd(&inputs[0]).dwFlags, Default::default());
    }

    #[test]
    fn a_key_up_sets_keyeventf_keyup() {
        let inputs = to_input(&InputEvent::Keyboard(KeyboardEvent::KeyUp {
            key: "RETURN".to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(as_keybd(&inputs[0]).wVk, VK_RETURN);
        assert_eq!(as_keybd(&inputs[0]).dwFlags, KEYEVENTF_KEYUP);
    }

    #[test]
    fn a_function_key_round_trips_arithmetically() {
        let inputs = to_input(&InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: "F13".to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(as_keybd(&inputs[0]).wVk.0, VK_F1.0 + 12);
    }

    #[test]
    fn a_hex_fallback_key_round_trips_to_its_raw_code() {
        let inputs = to_input(&InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: "0x1234".to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(as_keybd(&inputs[0]).wVk.0, 0x1234);
    }

    #[test]
    fn an_unknown_key_name_translates_to_nothing() {
        assert!(to_input(&InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: "NOT_A_REAL_KEY".to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        }))
        .is_none());
    }

    #[test]
    fn mouse_move_carries_its_delta_with_a_relative_move_flag() {
        let inputs = to_input(&InputEvent::Mouse(MouseEvent::Move {
            dx: 5,
            dy: -3,
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].r#type, INPUT_MOUSE);
        let mi = as_mouse(&inputs[0]);
        assert_eq!((mi.dx, mi.dy), (5, -3));
        assert_eq!(mi.dwFlags, MOUSEEVENTF_MOVE);
    }

    #[test]
    fn vertical_scroll_uses_wheel_flag_and_denormalizes_by_wheel_delta() {
        let inputs = to_input(&InputEvent::Mouse(MouseEvent::Scroll {
            dx: 0,
            dy: 2,
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(inputs.len(), 1);
        let mi = as_mouse(&inputs[0]);
        assert_eq!(mi.dwFlags, MOUSEEVENTF_WHEEL);
        assert_eq!(mi.mouseData as i32, 2 * WHEEL_DELTA as i32);
    }

    #[test]
    fn horizontal_scroll_uses_hwheel_flag() {
        let inputs = to_input(&InputEvent::Mouse(MouseEvent::Scroll {
            dx: -1,
            dy: 0,
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(inputs.len(), 1);
        let mi = as_mouse(&inputs[0]);
        assert_eq!(mi.dwFlags, MOUSEEVENTF_HWHEEL);
        assert_eq!(mi.mouseData as i32, -(WHEEL_DELTA as i32));
    }

    #[test]
    fn a_scroll_with_both_axes_produces_two_inputs() {
        let inputs = to_input(&InputEvent::Mouse(MouseEvent::Scroll {
            dx: 1,
            dy: 1,
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(inputs.len(), 2);
        assert_eq!(as_mouse(&inputs[0]).dwFlags, MOUSEEVENTF_WHEEL);
        assert_eq!(as_mouse(&inputs[1]).dwFlags, MOUSEEVENTF_HWHEEL);
    }

    #[test]
    fn mouse_buttons_map_to_their_dedicated_flags() {
        let down = to_input(&InputEvent::Mouse(MouseEvent::ButtonDown {
            button: MouseButton::Right,
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(as_mouse(&down[0]).dwFlags, MOUSEEVENTF_RIGHTDOWN);

        let up = to_input(&InputEvent::Mouse(MouseEvent::ButtonUp {
            button: MouseButton::Middle,
            timestamp_ms: 0,
        }))
        .unwrap();
        assert_eq!(as_mouse(&up[0]).dwFlags, MOUSEEVENTF_MIDDLEUP);
    }
}
