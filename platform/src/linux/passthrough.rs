//! Local pass-through while capture holds an exclusive `EVIOCGRAB`.
//!
//! A grabbed device delivers nothing to the local session, so an event the
//! router answers [`Disposition::Pass`] for would be lost (review H-08).
//! [`PassThrough`] decides, without any device I/O, which events must be
//! re-emitted through the Flow virtual (uinput) device so the local session
//! still sees them, and which release events repair key state across a
//! grab or ungrab. The capture loop owns the actual injector.

use std::collections::HashMap;

use flow_core::input::Disposition;
use flow_core::protocol::{InputEvent, KeyboardEvent, MouseButton, MouseEvent};

use super::inject_translate::key_code_for;

#[derive(Debug, Default)]
pub(super) struct PassThrough {
    /// Keys/buttons physically held now, mapped to the event that releases
    /// them.
    held: HashMap<String, InputEvent>,
    /// Keys/buttons the virtual device currently holds down because a
    /// `Pass` press was re-emitted; they must be released on ungrab, as
    /// the physical release will then bypass the virtual device.
    emitted: HashMap<String, InputEvent>,
}

impl PassThrough {
    /// Records the physical state change of `event` (call for every
    /// captured event, whatever its disposition).
    pub fn observe(&mut self, event: &InputEvent) {
        if let Some((id, down, release)) = key_state(event) {
            if down {
                self.held.insert(id, release);
            } else {
                self.held.remove(&id);
            }
        }
    }

    /// The event to re-emit locally for `event`, if any: only while the
    /// device is `grabbed` and the sink answered `Pass`.
    pub fn forward(
        &mut self,
        event: &InputEvent,
        disposition: Disposition,
        grabbed: bool,
    ) -> Option<InputEvent> {
        if !grabbed || disposition != Disposition::Pass {
            return None;
        }
        if let Some((id, down, release)) = key_state(event) {
            if down {
                self.emitted.insert(id, release);
            } else {
                self.emitted.remove(&id);
            }
        }
        Some(event.clone())
    }

    /// Events to emit when the grab starts, so keys already held (the
    /// switch chord's modifiers) do not stay down in the local session.
    /// The virtual device never saw those presses and the input core drops
    /// a release for a key it holds up, so modifiers get a press then a
    /// release; other keys and buttons only a release, because a synthetic
    /// press would type or click.
    pub fn on_grab(&mut self) -> Vec<InputEvent> {
        let mut out = Vec::new();
        for (id, release) in &self.held {
            if !id.starts_with("btn:") && is_modifier(release) {
                out.push(press_of(release));
            }
            out.push(release.clone());
        }
        out
    }

    /// Releases for everything the virtual device still holds down, when
    /// the grab ends.
    pub fn on_ungrab(&mut self) -> Vec<InputEvent> {
        self.emitted.drain().map(|(_, release)| release).collect()
    }
}

/// `(identity, is_press, releasing event)` for key and button events.
fn key_state(event: &InputEvent) -> Option<(String, bool, InputEvent)> {
    match event {
        InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key,
            modifiers,
            timestamp_ms,
        }) => Some((key.clone(), true, key_up(key, modifiers, *timestamp_ms))),
        InputEvent::Keyboard(KeyboardEvent::KeyUp { key, .. }) => {
            Some((key.clone(), false, event.clone()))
        }
        InputEvent::Mouse(MouseEvent::ButtonDown {
            button,
            timestamp_ms,
        }) => Some((
            button_id(*button),
            true,
            InputEvent::Mouse(MouseEvent::ButtonUp {
                button: *button,
                timestamp_ms: *timestamp_ms,
            }),
        )),
        InputEvent::Mouse(MouseEvent::ButtonUp { button, .. }) => {
            Some((button_id(*button), false, event.clone()))
        }
        _ => None,
    }
}

fn key_up(key: &str, modifiers: &[flow_core::protocol::Modifier], timestamp_ms: u64) -> InputEvent {
    InputEvent::Keyboard(KeyboardEvent::KeyUp {
        key: key.to_owned(),
        modifiers: modifiers.to_vec(),
        timestamp_ms,
    })
}

fn button_id(button: MouseButton) -> String {
    format!("btn:{button:?}")
}

fn is_modifier(release: &InputEvent) -> bool {
    use evdev::KeyCode;
    let InputEvent::Keyboard(KeyboardEvent::KeyUp { key, .. }) = release else {
        return false;
    };
    matches!(
        key_code_for(key),
        Some(
            KeyCode::KEY_LEFTSHIFT
                | KeyCode::KEY_RIGHTSHIFT
                | KeyCode::KEY_LEFTCTRL
                | KeyCode::KEY_RIGHTCTRL
                | KeyCode::KEY_LEFTALT
                | KeyCode::KEY_RIGHTALT
                | KeyCode::KEY_LEFTMETA
                | KeyCode::KEY_RIGHTMETA
        )
    )
}

fn press_of(release: &InputEvent) -> InputEvent {
    match release {
        InputEvent::Keyboard(KeyboardEvent::KeyUp {
            key,
            modifiers,
            timestamp_ms,
        }) => InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: key.clone(),
            modifiers: modifiers.clone(),
            timestamp_ms: *timestamp_ms,
        }),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn down(key: &str) -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: key.into(),
            modifiers: vec![],
            timestamp_ms: 1,
        })
    }
    fn up(key: &str) -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyUp {
            key: key.into(),
            modifiers: vec![],
            timestamp_ms: 1,
        })
    }
    fn mv() -> InputEvent {
        InputEvent::Mouse(MouseEvent::Move {
            dx: 1,
            dy: 1,
            timestamp_ms: 3,
        })
    }

    #[test]
    fn forwards_only_pass_while_grabbed() {
        let mut p = PassThrough::default();
        assert!(p.forward(&mv(), Disposition::Pass, false).is_none());
        assert!(p.forward(&mv(), Disposition::Swallow, true).is_none());
        assert_eq!(p.forward(&mv(), Disposition::Pass, true), Some(mv()));
    }

    #[test]
    fn grab_presses_and_releases_held_modifiers_but_only_releases_others() {
        let mut p = PassThrough::default();
        p.observe(&down("LEFTCTRL"));
        p.observe(&down("A"));
        let out = p.on_grab();
        assert_eq!(out.len(), 3);
        assert!(out.contains(&down("LEFTCTRL")));
        assert!(out.contains(&up("LEFTCTRL")));
        assert!(out.contains(&up("A")));
        assert!(!out.contains(&down("A")));
    }

    #[test]
    fn released_keys_are_not_released_on_grab() {
        let mut p = PassThrough::default();
        p.observe(&down("LEFTCTRL"));
        p.observe(&up("LEFTCTRL"));
        assert!(p.on_grab().is_empty());
    }

    #[test]
    fn ungrab_releases_keys_pressed_through_the_virtual_device() {
        let mut p = PassThrough::default();
        p.forward(&down("B"), Disposition::Pass, true);
        p.forward(&down("C"), Disposition::Pass, true);
        p.forward(&up("C"), Disposition::Pass, true);
        assert_eq!(p.on_ungrab(), vec![up("B")]);
        assert!(p.on_ungrab().is_empty());
    }

    #[test]
    fn mouse_button_held_at_grab_gets_release_only() {
        let mut p = PassThrough::default();
        let press = InputEvent::Mouse(MouseEvent::ButtonDown {
            button: MouseButton::Left,
            timestamp_ms: 1,
        });
        p.observe(&press);
        let out = p.on_grab();
        assert_eq!(out.len(), 1);
        assert!(matches!(
            out[0],
            InputEvent::Mouse(MouseEvent::ButtonUp { .. })
        ));
    }
}
