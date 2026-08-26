//! Platform-independent keyboard and mouse event model.
//!
//! See vision.md §11 (Input Event Protocol): events must stay independent
//! of operating system, transport, and UI so any of those can be swapped
//! without touching the others.

use serde::{Deserialize, Serialize};

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
    ///
    /// Capture-time metadata only — deliberately *not* what replay
    /// protection is keyed on. That's `ChannelMessage::Input`'s separate
    /// per-connection `sequence` (see its doc comment for why a
    /// wall-clock timestamp can't do the job: two legitimate
    /// high-frequency events can share a millisecond on a coarse OS
    /// clock).
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
