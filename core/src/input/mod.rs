//! Capture/injection traits that isolate OS-specific input handling
//! (vision.md §7, Flutter Does Not Remove OS Complexity).
//!
//! The `flow-platform` crate implements these per operating system; the
//! daemon's core loop depends only on the traits, never on a concrete
//! adapter.

use crate::protocol::InputEvent;

pub trait InputCapture {
    type Error;

    fn start(&mut self) -> Result<(), Self::Error>;
    fn stop(&mut self) -> Result<(), Self::Error>;

    /// Suppresses (or restores) delivery of captured input to *this*
    /// machine's own applications.
    ///
    /// This is what makes remote control actually remote. Capture is
    /// passive on every platform — an evdev read, a listen-only event
    /// tap, a low-level hook that always chains onward — so while
    /// another device is the active one, forwarding alone would deliver
    /// each keystroke to *both* machines. `vision.md` §22 is explicit
    /// that "only the active device should receive input," which means
    /// the forwarding side has to stop its own OS from seeing the same
    /// events.
    ///
    /// Deliberately has no default implementation. Every current adapter
    /// (Linux `EVIOCGRAB`, Windows low-level-hook swallow, macOS active
    /// `CGEventTap`) implements it for real; an adapter that genuinely
    /// could not should return an error rather than silently report
    /// success, since a caller that believes it suppressed local input
    /// when it didn't produces duplicated keystrokes the user never asked
    /// for — worse than a loud "this platform can't do that." The macOS
    /// and Windows implementations are unverified on real hardware (see
    /// `daemon/README.md`'s "Local input suppression" section).
    fn set_suppress_local(&mut self, suppress: bool) -> Result<(), Self::Error>;
}

pub trait InputInjector {
    type Error;

    fn inject(&mut self, event: &InputEvent) -> Result<(), Self::Error>;
}
