//! Platform-independent keyboard and mouse event model.
//!
//! See vision.md §11 (Input Event Protocol): events must stay independent
//! of operating system, transport, and UI so any of those can be swapped
//! without touching the others.

use serde::{Deserialize, Serialize};

pub mod key_names;

/// Which side of a paired connection this daemon's physical input
/// currently drives (`docs/product/vision.md` §22, "only the active
/// device should receive input").
///
/// `Primary` = this machine's keyboard/mouse are being captured and
/// forwarded to the peer (and suppressed locally). `Secondary` = this
/// machine is receiving and injecting the peer's input instead. Exactly
/// one end of a pair is `Primary` at any moment; a switch hands the role
/// across over the same connection, carried by
/// [`crate::channel::ChannelMessage::SwitchOwnership`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputRole {
    Primary,
    Secondary,
}

impl InputRole {
    /// The other role — a switch always flips this end to `opposite()` of
    /// what it was, and tells the peer to take `opposite()` of the new
    /// local role.
    pub fn opposite(self) -> Self {
        match self {
            InputRole::Primary => InputRole::Secondary,
            InputRole::Secondary => InputRole::Primary,
        }
    }
}

/// A keyboard modifier held down during a key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Modifier {
    Shift,
    Ctrl,
    Alt,
    Meta,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum KeyboardEvent {
    KeyDown {
        key: String,
        modifiers: Vec<Modifier>,
        timestamp_ms: u64,
    },
    KeyUp {
        key: String,
        modifiers: Vec<Modifier>,
        timestamp_ms: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MouseEvent {
    Move {
        dx: i32,
        dy: i32,
        timestamp_ms: u64,
    },
    ButtonDown {
        button: MouseButton,
        timestamp_ms: u64,
    },
    ButtonUp {
        button: MouseButton,
        timestamp_ms: u64,
    },
    Scroll {
        dx: i32,
        dy: i32,
        timestamp_ms: u64,
    },
}

/// A single input event as it travels between daemons, independent of the
/// transport carrying it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InputEvent {
    Keyboard(KeyboardEvent),
    Mouse(MouseEvent),
}

impl InputEvent {
    /// This event's own capture-time timestamp, common to every variant.
    /// `daemon/todos.json` H4's replay guard reuses this existing field
    /// as its sequence check rather than adding a separate one.
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            InputEvent::Keyboard(KeyboardEvent::KeyDown { timestamp_ms, .. })
            | InputEvent::Keyboard(KeyboardEvent::KeyUp { timestamp_ms, .. })
            | InputEvent::Mouse(MouseEvent::Move { timestamp_ms, .. })
            | InputEvent::Mouse(MouseEvent::ButtonDown { timestamp_ms, .. })
            | InputEvent::Mouse(MouseEvent::ButtonUp { timestamp_ms, .. })
            | InputEvent::Mouse(MouseEvent::Scroll { timestamp_ms, .. }) => *timestamp_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_role_opposite_is_the_other_role() {
        assert_eq!(InputRole::Primary.opposite(), InputRole::Secondary);
        assert_eq!(InputRole::Secondary.opposite(), InputRole::Primary);
    }

    #[test]
    fn input_role_serializes_snake_case() {
        assert_eq!(
            serde_json::to_value(InputRole::Primary).unwrap(),
            serde_json::json!("primary")
        );
        assert_eq!(
            serde_json::to_value(InputRole::Secondary).unwrap(),
            serde_json::json!("secondary")
        );
    }

    #[test]
    fn timestamp_ms_reads_the_right_field_on_every_variant() {
        assert_eq!(
            InputEvent::Keyboard(KeyboardEvent::KeyDown {
                key: "A".to_string(),
                modifiers: vec![],
                timestamp_ms: 1,
            })
            .timestamp_ms(),
            1
        );
        assert_eq!(
            InputEvent::Keyboard(KeyboardEvent::KeyUp {
                key: "A".to_string(),
                modifiers: vec![],
                timestamp_ms: 2,
            })
            .timestamp_ms(),
            2
        );
        assert_eq!(
            InputEvent::Mouse(MouseEvent::Move {
                dx: 1,
                dy: 1,
                timestamp_ms: 3,
            })
            .timestamp_ms(),
            3
        );
        assert_eq!(
            InputEvent::Mouse(MouseEvent::ButtonDown {
                button: MouseButton::Left,
                timestamp_ms: 4,
            })
            .timestamp_ms(),
            4
        );
        assert_eq!(
            InputEvent::Mouse(MouseEvent::ButtonUp {
                button: MouseButton::Left,
                timestamp_ms: 5,
            })
            .timestamp_ms(),
            5
        );
        assert_eq!(
            InputEvent::Mouse(MouseEvent::Scroll {
                dx: 1,
                dy: 1,
                timestamp_ms: 6,
            })
            .timestamp_ms(),
            6
        );
    }
}
