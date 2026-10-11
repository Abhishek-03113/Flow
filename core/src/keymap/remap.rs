//! User remap rules, applied on the controller before an event is
//! forwarded (e.g. swap Ctrl and Meta when controlling a Mac).
//!
//! A rule's `from`/`to` are either two key names from the shared
//! vocabulary (`CAPS_LOCK` -> `ESCAPE`) or two modifier tokens (`Ctrl`,
//! `Alt`, `Shift`, `Meta`). A modifier rule remaps both the left and right
//! key of that modifier and the event's `modifiers` list; a key-name rule
//! remaps only that key. Rules never chain: each key/modifier is looked up
//! once against the original event, so two rules can swap.

use serde::{Deserialize, Serialize};

use crate::device::HostOs;
use crate::protocol::{key_names, InputEvent, KeyboardEvent, Modifier};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemapRule {
    pub from: String,
    pub to: String,
    /// Only apply when forwarding to this OS; `None` applies to all.
    #[serde(default)]
    pub when_target_os: Option<HostOs>,
}

/// The on-disk shape of `remap.toml`: a list of `[[rule]]` tables.
#[derive(Debug, Default, Serialize, Deserialize)]
struct RemapFile {
    #[serde(default)]
    rule: Vec<RemapRule>,
}

#[derive(Debug, thiserror::Error)]
pub enum RemapError {
    #[error("invalid TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("rule {from:?} -> {to:?}: {reason}")]
    Invalid {
        from: String,
        to: String,
        reason: &'static str,
    },
    #[error("more than one rule for {from:?} on the same computer")]
    Duplicate { from: String },
}

enum Target {
    Modifier(Modifier),
    Key(String),
}

fn classify(token: &str) -> Option<Target> {
    match token {
        "Shift" => Some(Target::Modifier(Modifier::Shift)),
        "Ctrl" => Some(Target::Modifier(Modifier::Ctrl)),
        "Alt" => Some(Target::Modifier(Modifier::Alt)),
        "Meta" => Some(Target::Modifier(Modifier::Meta)),
        // The UI's tokens (`CapsLock`, `Esc`) and wire names (`CAPS_LOCK`) alike.
        name => key_names::canonical(name).map(Target::Key),
    }
}

/// The (left, right) key names of a modifier.
fn modifier_keys(modifier: Modifier) -> (&'static str, &'static str) {
    match modifier {
        Modifier::Shift => (key_names::SHIFT, key_names::RIGHT_SHIFT),
        Modifier::Ctrl => (key_names::CONTROL, key_names::RIGHT_CONTROL),
        Modifier::Alt => (key_names::OPTION, key_names::RIGHT_OPTION),
        Modifier::Meta => (key_names::COMMAND, key_names::RIGHT_COMMAND),
    }
}

impl RemapRule {
    /// Both sides must be known, and of the same kind (modifier or key).
    pub fn validate(&self) -> Result<(), RemapError> {
        let invalid = |reason| RemapError::Invalid {
            from: self.from.clone(),
            to: self.to.clone(),
            reason,
        };
        match (classify(&self.from), classify(&self.to)) {
            (None, _) | (_, None) => Err(invalid("unknown key or modifier name")),
            (Some(Target::Modifier(_)), Some(Target::Key(_)))
            | (Some(Target::Key(_)), Some(Target::Modifier(_))) => {
                Err(invalid("cannot map a modifier to a key or the reverse"))
            }
            _ => Ok(()),
        }
    }
}

/// Validates a whole rule set: every rule on its own, and no two rules for
/// the same key or modifier and the same `when_target_os` (only the first
/// would ever apply).
pub fn validate_rules(rules: &[RemapRule]) -> Result<(), RemapError> {
    let mut seen: Vec<(String, Option<HostOs>)> = Vec::with_capacity(rules.len());
    for rule in rules {
        rule.validate()?;
        let from = match classify(&rule.from) {
            Some(Target::Modifier(m)) => format!("{m:?}"),
            Some(Target::Key(k)) => k,
            None => rule.from.clone(),
        };
        let key = (from, rule.when_target_os);
        if seen.contains(&key) {
            return Err(RemapError::Duplicate {
                from: rule.from.clone(),
            });
        }
        seen.push(key);
    }
    Ok(())
}

/// Parses a `remap.toml` and validates every rule.
pub fn parse_rules(text: &str) -> Result<Vec<RemapRule>, RemapError> {
    let file: RemapFile = toml::from_str(text)?;
    file.rule.iter().try_for_each(RemapRule::validate)?;
    Ok(file.rule)
}

/// Renders rules as the `remap.toml` shape [`parse_rules`] reads.
pub fn rules_to_toml(rules: &[RemapRule]) -> String {
    let file = RemapFile {
        rule: rules.to_vec(),
    };
    toml::to_string(&file).expect("remap rules serialize to TOML")
}

/// Rewrites a keyboard event's key and modifiers for `target_os`. Mouse
/// events and events no rule matches pass through unchanged.
pub fn apply_remaps(event: InputEvent, target_os: HostOs, rules: &[RemapRule]) -> InputEvent {
    let InputEvent::Keyboard(keyboard) = event else {
        return event;
    };
    let applicable = || {
        rules
            .iter()
            .filter(move |r| r.when_target_os.is_none_or(|os| os == target_os))
    };
    let remap_key = |key: String| -> String {
        for rule in applicable() {
            match (classify(&rule.from), classify(&rule.to)) {
                (Some(Target::Key(from)), Some(Target::Key(to))) if from == key => return to,
                (Some(Target::Modifier(from)), Some(Target::Modifier(to))) => {
                    let (from_left, from_right) = modifier_keys(from);
                    let (to_left, to_right) = modifier_keys(to);
                    if key == from_left {
                        return to_left.to_string();
                    }
                    if key == from_right {
                        return to_right.to_string();
                    }
                }
                _ => {}
            }
        }
        key
    };
    let remap_modifiers = |modifiers: Vec<Modifier>| -> Vec<Modifier> {
        let mapped: Vec<Modifier> = modifiers
            .into_iter()
            .map(|modifier| {
                applicable()
                    .find_map(|rule| match (classify(&rule.from), classify(&rule.to)) {
                        (Some(Target::Modifier(from)), Some(Target::Modifier(to)))
                            if from == modifier =>
                        {
                            Some(to)
                        }
                        _ => None,
                    })
                    .unwrap_or(modifier)
            })
            .collect();
        // Fixed order, deduplicated — same shape the capture side emits.
        [
            Modifier::Shift,
            Modifier::Ctrl,
            Modifier::Alt,
            Modifier::Meta,
        ]
        .into_iter()
        .filter(|m| mapped.contains(m))
        .collect()
    };
    InputEvent::Keyboard(match keyboard {
        KeyboardEvent::KeyDown {
            key,
            modifiers,
            timestamp_ms,
        } => KeyboardEvent::KeyDown {
            key: remap_key(key),
            modifiers: remap_modifiers(modifiers),
            timestamp_ms,
        },
        KeyboardEvent::KeyUp {
            key,
            modifiers,
            timestamp_ms,
        } => KeyboardEvent::KeyUp {
            key: remap_key(key),
            modifiers: remap_modifiers(modifiers),
            timestamp_ms,
        },
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn rules_written_with_the_ui_tokens_apply_to_wire_keys() {
        let rules = vec![RemapRule {
            from: "CapsLock".to_string(),
            to: "Esc".to_string(),
            when_target_os: None,
        }];
        rules[0].validate().expect("UI tokens are valid");
        let down = InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: key_names::CAPS_LOCK.to_string(),
            modifiers: vec![],
            timestamp_ms: 1,
        });
        let InputEvent::Keyboard(KeyboardEvent::KeyDown { key, .. }) =
            apply_remaps(down, HostOs::Macos, &rules)
        else {
            panic!("still a key down");
        };
        assert_eq!(key, key_names::ESCAPE);
    }

    #[test]
    fn two_rules_for_one_key_and_computer_are_refused() {
        let caps = |to: &str, os| rule("CapsLock", to, os);
        assert!(matches!(
            validate_rules(&[caps("F11", None), caps("F12", None)]),
            Err(RemapError::Duplicate { .. })
        ));
        // `CAPS_LOCK` is the same key as `CapsLock`.
        assert!(matches!(
            validate_rules(&[caps("F11", None), rule("CAPS_LOCK", "F12", None)]),
            Err(RemapError::Duplicate { .. })
        ));
        assert!(matches!(
            validate_rules(&[rule("Ctrl", "Meta", None), rule("Ctrl", "Alt", None)]),
            Err(RemapError::Duplicate { .. })
        ));
        // A different target OS is a different rule.
        assert!(validate_rules(&[
            caps("F11", None),
            caps("F12", Some(HostOs::Windows)),
            caps("End", Some(HostOs::Macos)),
        ])
        .is_ok());
        // Invalid rules are still reported as invalid.
        assert!(matches!(
            validate_rules(&[rule("Ctrl", "A", None)]),
            Err(RemapError::Invalid { .. })
        ));
    }

    #[test]
    fn rules_survive_a_round_trip_through_toml() {
        let rules = vec![RemapRule {
            from: "Ctrl".to_string(),
            to: "Meta".to_string(),
            when_target_os: Some(HostOs::Macos),
        }];
        assert_eq!(parse_rules(&rules_to_toml(&rules)).unwrap(), rules);
    }

    use super::*;
    use crate::protocol::MouseEvent;

    fn rule(from: &str, to: &str, os: Option<HostOs>) -> RemapRule {
        RemapRule {
            from: from.into(),
            to: to.into(),
            when_target_os: os,
        }
    }

    fn down(key: &str, modifiers: Vec<Modifier>) -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: key.into(),
            modifiers,
            timestamp_ms: 7,
        })
    }

    fn ctrl_meta_swap() -> Vec<RemapRule> {
        vec![
            rule("Ctrl", "Meta", Some(HostOs::Macos)),
            rule("Meta", "Ctrl", Some(HostOs::Macos)),
        ]
    }

    #[test]
    fn a_ctrl_meta_swap_remaps_keys_and_modifiers_for_a_mac_target() {
        let rules = ctrl_meta_swap();
        assert_eq!(
            apply_remaps(down("CONTROL", vec![Modifier::Ctrl]), HostOs::Macos, &rules),
            down("COMMAND", vec![Modifier::Meta])
        );
        assert_eq!(
            apply_remaps(
                down("RIGHT_COMMAND", vec![Modifier::Meta]),
                HostOs::Macos,
                &rules
            ),
            down("RIGHT_CONTROL", vec![Modifier::Ctrl])
        );
        // C with Ctrl held becomes C with Meta held: Ctrl+C -> Cmd+C.
        assert_eq!(
            apply_remaps(down("C", vec![Modifier::Ctrl]), HostOs::Macos, &rules),
            down("C", vec![Modifier::Meta])
        );
    }

    #[test]
    fn a_swap_does_not_chain() {
        let rules = ctrl_meta_swap();
        assert_eq!(
            apply_remaps(
                down("A", vec![Modifier::Ctrl, Modifier::Meta]),
                HostOs::Macos,
                &rules
            ),
            down("A", vec![Modifier::Ctrl, Modifier::Meta])
        );
    }

    #[test]
    fn rules_scoped_to_another_os_do_not_apply() {
        let rules = ctrl_meta_swap();
        let event = down("CONTROL", vec![Modifier::Ctrl]);
        assert_eq!(apply_remaps(event.clone(), HostOs::Windows, &rules), event);
    }

    #[test]
    fn unscoped_rules_apply_to_every_target() {
        let rules = [rule("CAPS_LOCK", "ESCAPE", None)];
        for os in [HostOs::Windows, HostOs::Macos, HostOs::Linux] {
            assert_eq!(
                apply_remaps(down("CAPS_LOCK", vec![]), os, &rules),
                down("ESCAPE", vec![])
            );
        }
    }

    #[test]
    fn a_key_rule_leaves_other_keys_and_modifiers_alone() {
        let rules = [rule("CAPS_LOCK", "ESCAPE", None)];
        let event = down("A", vec![Modifier::Shift]);
        assert_eq!(apply_remaps(event.clone(), HostOs::Linux, &rules), event);
    }

    #[test]
    fn key_up_and_mouse_events_are_handled() {
        let rules = ctrl_meta_swap();
        let up = InputEvent::Keyboard(KeyboardEvent::KeyUp {
            key: "CONTROL".into(),
            modifiers: vec![],
            timestamp_ms: 1,
        });
        assert_eq!(
            apply_remaps(up, HostOs::Macos, &rules),
            InputEvent::Keyboard(KeyboardEvent::KeyUp {
                key: "COMMAND".into(),
                modifiers: vec![],
                timestamp_ms: 1,
            })
        );
        let mouse = InputEvent::Mouse(MouseEvent::Move {
            dx: 1,
            dy: 2,
            timestamp_ms: 0,
        });
        assert_eq!(apply_remaps(mouse.clone(), HostOs::Macos, &rules), mouse);
    }

    #[test]
    fn parses_the_toml_form() {
        let rules = parse_rules(
            "[[rule]]\nfrom = \"Ctrl\"\nto = \"Meta\"\nwhen_target_os = \"macos\"\n\n[[rule]]\nfrom = \"CAPS_LOCK\"\nto = \"ESCAPE\"\n",
        )
        .unwrap();
        assert_eq!(
            rules,
            vec![
                rule("Ctrl", "Meta", Some(HostOs::Macos)),
                rule("CAPS_LOCK", "ESCAPE", None)
            ]
        );
        assert!(parse_rules("").unwrap().is_empty());
    }

    #[test]
    fn rejects_unknown_names_and_mixed_kinds() {
        assert!(matches!(
            parse_rules("[[rule]]\nfrom = \"NOPE\"\nto = \"A\"").unwrap_err(),
            RemapError::Invalid { .. }
        ));
        assert!(matches!(
            parse_rules("[[rule]]\nfrom = \"Ctrl\"\nto = \"COMMAND\"").unwrap_err(),
            RemapError::Invalid { .. }
        ));
    }

    #[test]
    fn rules_round_trip_through_json_for_set_remap_rules() {
        let rules = ctrl_meta_swap();
        let json = serde_json::to_string(&rules).unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<RemapRule>>(&json).unwrap(),
            rules
        );
        // `when_target_os` may be omitted or null.
        let parsed: Vec<RemapRule> = serde_json::from_str(
            r#"[{"from":"Ctrl","to":"Meta","when_target_os":null},{"from":"Alt","to":"Meta"}]"#,
        )
        .unwrap();
        assert!(parsed.iter().all(|r| r.when_target_os.is_none()));
    }
}
