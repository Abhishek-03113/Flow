//! The notation scenarios use for input events, and how observed events
//! are compared: keys by the *native code of the observing OS*, so a
//! scenario written once ("down A") is checked against every OS's keymap.

use std::fmt;

use flow_core::device::HostOs;
use flow_core::protocol::{InputEvent, KeyboardEvent, MouseButton, MouseEvent};
use serde::Deserialize;

use crate::keys::OsKeyboard;

/// One event as seen on a device, with keys resolved to that OS's code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    Key {
        down: bool,
        name: String,
        /// `None` when the OS has no key with this wire name.
        code: Option<u32>,
    },
    Move {
        dx: i32,
        dy: i32,
    },
    Button {
        down: bool,
        button: MouseButton,
    },
    Scroll {
        dx: i32,
        dy: i32,
    },
}

/// Filters events by family when a scenario cares about only some.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Key,
    Move,
    Button,
    Scroll,
}

impl Seen {
    pub fn kind(&self) -> Kind {
        match self {
            Seen::Key { .. } => Kind::Key,
            Seen::Move { .. } => Kind::Move,
            Seen::Button { .. } => Kind::Button,
            Seen::Scroll { .. } => Kind::Scroll,
        }
    }

    /// Two events are the same when their keys have the same native code
    /// (or, with no code on either side, the same name).
    pub fn same_as(&self, other: &Seen) -> bool {
        match (self, other) {
            (
                Seen::Key {
                    down: d1,
                    name: n1,
                    code: c1,
                },
                Seen::Key {
                    down: d2,
                    name: n2,
                    code: c2,
                },
            ) => {
                d1 == d2
                    && match (c1, c2) {
                        (Some(a), Some(b)) => a == b,
                        _ => n1 == n2 && c1 == c2,
                    }
            }
            _ => self == other,
        }
    }
}

impl fmt::Display for Seen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Seen::Key { down, name, code } => {
                let dir = if *down { "down" } else { "up" };
                match code {
                    Some(code) => write!(f, "{dir} {name} [{code:#04x}]"),
                    None => write!(f, "{dir} {name} [no such key]"),
                }
            }
            Seen::Move { dx, dy } => write!(f, "move {dx} {dy}"),
            Seen::Button { down, button } => {
                let dir = if *down { "button_down" } else { "button_up" };
                write!(f, "{dir} {}", button_name(*button))
            }
            Seen::Scroll { dx, dy } => write!(f, "scroll {dx} {dy}"),
        }
    }
}

fn button_name(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "left",
        MouseButton::Right => "right",
        MouseButton::Middle => "middle",
    }
}

pub fn parse_button(name: &str) -> Result<MouseButton, String> {
    match name {
        "left" => Ok(MouseButton::Left),
        "right" => Ok(MouseButton::Right),
        "middle" => Ok(MouseButton::Middle),
        other => Err(format!("unknown mouse button {other:?}")),
    }
}

/// Resolves an event the engine delivered or injected on a device of
/// `keyboard`'s OS.
pub fn observe(keyboard: &OsKeyboard, event: &InputEvent) -> Seen {
    match event {
        InputEvent::Keyboard(KeyboardEvent::KeyDown { key, .. }) => Seen::Key {
            down: true,
            code: keyboard.injected_code(key),
            name: key.clone(),
        },
        InputEvent::Keyboard(KeyboardEvent::KeyUp { key, .. }) => Seen::Key {
            down: false,
            code: keyboard.injected_code(key),
            name: key.clone(),
        },
        InputEvent::Mouse(MouseEvent::Move { dx, dy, .. }) => Seen::Move { dx: *dx, dy: *dy },
        InputEvent::Mouse(MouseEvent::ButtonDown { button, .. }) => Seen::Button {
            down: true,
            button: *button,
        },
        InputEvent::Mouse(MouseEvent::ButtonUp { button, .. }) => Seen::Button {
            down: false,
            button: *button,
        },
        InputEvent::Mouse(MouseEvent::Scroll { dx, dy, .. }) => Seen::Scroll { dx: *dx, dy: *dy },
    }
}

/// Parses a scenario's event notation as seen on a device of `keyboard`'s
/// OS: `down A`, `up A`, `move 10 5`, `button_down left`, `scroll 0 -3`.
/// A key the OS doesn't have is an error: the scenario can't hold there.
pub fn parse_expected(keyboard: &OsKeyboard, text: &str) -> Result<Seen, String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let number = |word: Option<&&str>| -> Result<i32, String> {
        word.ok_or_else(|| format!("{text:?}: missing number"))?
            .parse()
            .map_err(|_| format!("{text:?}: not a number"))
    };
    match words.as_slice() {
        [dir @ ("down" | "up"), key] => Ok(Seen::Key {
            down: *dir == "down",
            name: (*key).to_string(),
            code: Some(keyboard.native_code(key)?),
        }),
        ["move", ..] => Ok(Seen::Move {
            dx: number(words.get(1))?,
            dy: number(words.get(2))?,
        }),
        ["scroll", ..] => Ok(Seen::Scroll {
            dx: number(words.get(1))?,
            dy: number(words.get(2))?,
        }),
        [dir @ ("button_down" | "button_up"), button] => Ok(Seen::Button {
            down: *dir == "button_down",
            button: parse_button(button)?,
        }),
        _ => Err(format!("{text:?} is not an event (down|up KEY, move DX DY, button_down|button_up BTN, scroll DX DY)")),
    }
}

/// `os` for messages.
pub fn os_label(os: HostOs) -> &'static str {
    match os {
        HostOs::Windows => "windows",
        HostOs::Macos => "macos",
        HostOs::Linux => "linux",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expectations_compare_by_the_observers_native_code() {
        let win = OsKeyboard::new(HostOs::Windows);
        let injected = InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: "A".into(),
            modifiers: vec![],
            timestamp_ms: 1,
        });
        let seen = observe(&win, &injected);
        assert!(seen.same_as(&parse_expected(&win, "down A").unwrap()));
        assert!(!seen.same_as(&parse_expected(&win, "up A").unwrap()));
        assert_eq!(seen.to_string(), "down A [0x41]");
    }

    #[test]
    fn a_key_the_os_lacks_cannot_be_expected() {
        let win = OsKeyboard::new(HostOs::Windows);
        assert!(parse_expected(&win, "down FUNCTION").is_err());
    }
}
