//! Platform-independent keyboard and mouse event model.
//!
//! See vision.md §11 (Input Event Protocol): events must stay independent
//! of operating system, transport, and UI so any of those can be swapped
//! without touching the others.

use serde::{Deserialize, Serialize};

/// A keyboard modifier held down during a key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
