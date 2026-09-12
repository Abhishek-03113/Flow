//! The canonical `KeyboardEvent.key` vocabulary every platform adapter in
//! `flow-platform` must encode to (capture, `translate.rs`) and decode
//! from (inject, `inject_translate.rs`).
//!
//! Before this module existed, each of the three platforms invented its
//! own key-name strings independently — Windows sent `"LMENU"`, macOS's
//! inject side only recognized `"OPTION"`. A name that didn't match the
//! receiving platform's whitelist, and didn't parse as that platform's
//! own `"0xNN"` hex fallback, was silently dropped: the daemon logged
//! `stage="injected"` while nothing reached the OS. Sharing one set of
//! `const` names, checked by the compiler on every platform that
//! references them, is what keeps that from happening again — a typo or
//! a missed key here is a compile error in the platform crate, not a
//! silent no-op discovered by testing two physical machines together.
//!
//! ## The vocabulary
//!
//! - Plain letters (`'A'..='Z'`) and digits (`'0'..='9'`) are **not**
//!   constants here: every platform already reports (and must keep
//!   reporting) them as the bare single-character string, e.g. `"A"`,
//!   `"5"`. That convention predates this module and is left alone.
//! - `F1` through `F20` follow the same `format!("F{n}")` shape used
//!   below in [`function_key`] — also not literal constants, since
//!   they're entirely regular.
//! - Everything else Flow currently needs to move across platforms is a
//!   `pub const` below, named after macOS's own vocabulary (the most
//!   descriptive and most complete of the three prior, independent
//!   naming schemes).
//!
//! A platform's own hardware-specific keys with no cross-platform
//! equivalent (Windows' menu key, say) are *not* expected to appear
//! here — they keep whatever platform-local name `translate.rs` already
//! gives them. They simply won't match on another OS, which is correct:
//! there is nothing there to match.

/// One name per `F1..=F20`, e.g. `function_key(13) == "F13"`. `F21..=F24`
/// exist on some Windows/Linux keyboards but have no macOS equivalent
/// (`core_graphics::event::KeyCode` stops at `F20`), so they're left out
/// of the shared vocabulary — a platform that captures them keeps its
/// own name, same as any other platform-specific key.
pub fn function_key(n: u8) -> String {
    format!("F{n}")
}

pub const RETURN: &str = "RETURN";
pub const TAB: &str = "TAB";
pub const SPACE: &str = "SPACE";
/// The "delete"/backspace key immediately after the last letter row —
/// what macOS itself calls `DELETE` even though it deletes backward.
pub const DELETE: &str = "DELETE";
/// The dedicated forward-delete ("Del") key some keyboards have in
/// addition to the one above.
pub const FORWARD_DELETE: &str = "FORWARD_DELETE";
pub const ESCAPE: &str = "ESCAPE";

pub const SHIFT: &str = "SHIFT";
pub const RIGHT_SHIFT: &str = "RIGHT_SHIFT";
pub const CONTROL: &str = "CONTROL";
pub const RIGHT_CONTROL: &str = "RIGHT_CONTROL";
/// The Alt/Option key position.
pub const OPTION: &str = "OPTION";
pub const RIGHT_OPTION: &str = "RIGHT_OPTION";
/// The Windows/Super/Command key position.
pub const COMMAND: &str = "COMMAND";
pub const RIGHT_COMMAND: &str = "RIGHT_COMMAND";
pub const CAPS_LOCK: &str = "CAPS_LOCK";
pub const FUNCTION: &str = "FUNCTION";

pub const HOME: &str = "HOME";
pub const END: &str = "END";
pub const PAGE_UP: &str = "PAGE_UP";
pub const PAGE_DOWN: &str = "PAGE_DOWN";
pub const LEFT_ARROW: &str = "LEFT_ARROW";
pub const RIGHT_ARROW: &str = "RIGHT_ARROW";
pub const UP_ARROW: &str = "UP_ARROW";
pub const DOWN_ARROW: &str = "DOWN_ARROW";
pub const HELP: &str = "HELP";

pub const VOLUME_UP: &str = "VOLUME_UP";
pub const VOLUME_DOWN: &str = "VOLUME_DOWN";
pub const MUTE: &str = "MUTE";

// Punctuation. Previously unhandled by any platform's named table (all
// three fell back to a platform-specific raw hex code for these), so
// there was no prior vocabulary to preserve — these names are new.
pub const MINUS: &str = "MINUS";
pub const EQUAL: &str = "EQUAL";
pub const LEFT_BRACKET: &str = "LEFT_BRACKET";
pub const RIGHT_BRACKET: &str = "RIGHT_BRACKET";
pub const SEMICOLON: &str = "SEMICOLON";
pub const QUOTE: &str = "QUOTE";
pub const COMMA: &str = "COMMA";
pub const PERIOD: &str = "PERIOD";
pub const SLASH: &str = "SLASH";
pub const BACKSLASH: &str = "BACKSLASH";
pub const GRAVE: &str = "GRAVE";

/// Every constant above, plus `'A'..='Z'`/`'0'..='9'` and `F1..=F20` —
/// the full set a platform's `code_for_name`/`key_code_for` (inject
/// side) must accept. Shared by each platform's own parity test
/// (`platform/src/*/inject_translate.rs`) so the list itself only needs
/// maintaining in one place.
pub fn all() -> Vec<String> {
    let mut names: Vec<String> = vec![
        RETURN,
        TAB,
        SPACE,
        DELETE,
        FORWARD_DELETE,
        ESCAPE,
        SHIFT,
        RIGHT_SHIFT,
        CONTROL,
        RIGHT_CONTROL,
        OPTION,
        RIGHT_OPTION,
        COMMAND,
        RIGHT_COMMAND,
        CAPS_LOCK,
        FUNCTION,
        HOME,
        END,
        PAGE_UP,
        PAGE_DOWN,
        LEFT_ARROW,
        RIGHT_ARROW,
        UP_ARROW,
        DOWN_ARROW,
        HELP,
        VOLUME_UP,
        VOLUME_DOWN,
        MUTE,
        MINUS,
        EQUAL,
        LEFT_BRACKET,
        RIGHT_BRACKET,
        SEMICOLON,
        QUOTE,
        COMMA,
        PERIOD,
        SLASH,
        BACKSLASH,
        GRAVE,
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    names.extend(('A'..='Z').map(String::from));
    names.extend(('0'..='9').map(String::from));
    names.extend((1..=20).map(function_key));
    names
}
