//! Debounces repeated switch-key matches (`daemon/todos.json` F3).
//!
//! The daemon now reads raw, possibly-repeating key events directly —
//! unlike the Dart mock, whose UI-visible debounce is a client concern
//! per `docs/contracts/daemon-ipc.md`'s "Switching" section ("a
//! debounced/disabled row while the command's Future is in flight is a
//! UI concern, not a wire state"). Holding the switch key (autorepeat)
//! or a noisy multi-key combo release must not fire more than one
//! switch per press.

use std::time::{Duration, Instant};

/// The window within which a second switch-key match is ignored,
/// counted from the previous *fired* match — not from every match
/// attempt, so holding the key down doesn't push the window out
/// indefinitely and never let a second switch through.
pub const SWITCH_DEBOUNCE_WINDOW: Duration = Duration::from_millis(500);

/// Tracks when a switch last actually fired, deciding whether a new
/// match should be allowed to fire another one. Takes the current time
/// as an explicit argument rather than reading a clock itself, so it's
/// deterministically unit-testable without needing real (or
/// tokio-paused) time.
pub struct SwitchDebouncer {
    window: Duration,
    last_fired: Option<Instant>,
}

impl SwitchDebouncer {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            last_fired: None,
        }
    }

    /// Call once per matcher match. Returns `true` the first time, and
    /// again only once `window` has elapsed since the last time this
    /// returned `true`.
    pub fn should_fire(&mut self, now: Instant) -> bool {
        let elapsed_enough = self
            .last_fired
            .is_none_or(|last| now.duration_since(last) >= self.window);
        if elapsed_enough {
            self.last_fired = Some(now);
        }
        elapsed_enough
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rapid_fire_matches_within_the_window_produce_exactly_one_fire() {
        let mut debouncer = SwitchDebouncer::new(Duration::from_millis(500));
        let t0 = Instant::now();

        assert!(debouncer.should_fire(t0));
        assert!(!debouncer.should_fire(t0 + Duration::from_millis(1)));
        assert!(!debouncer.should_fire(t0 + Duration::from_millis(250)));
        assert!(!debouncer.should_fire(t0 + Duration::from_millis(499)));
    }

    #[test]
    fn a_match_at_or_past_the_window_fires_again() {
        let mut debouncer = SwitchDebouncer::new(Duration::from_millis(500));
        let t0 = Instant::now();

        assert!(debouncer.should_fire(t0));
        assert!(debouncer.should_fire(t0 + Duration::from_millis(500)));
    }

    #[test]
    fn the_window_resets_from_the_most_recent_fire_not_the_first() {
        let mut debouncer = SwitchDebouncer::new(Duration::from_millis(500));
        let t0 = Instant::now();

        assert!(debouncer.should_fire(t0));
        assert!(debouncer.should_fire(t0 + Duration::from_millis(500)));
        // Only 100ms after the second fire, not 600ms after the first.
        assert!(!debouncer.should_fire(t0 + Duration::from_millis(600)));
    }
}
