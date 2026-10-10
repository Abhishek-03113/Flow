//! OS input permission state (`data-model.md` "PermissionStatus").
//!
//! The engine reports what the OS requires as a list of [`PermissionItem`]s
//! (macOS: Accessibility and Input Monitoring, separately; Linux: reading
//! input devices and creating the virtual one; Windows: none to grant).
//! Names and fix-it text are engine-supplied, so the UI never hardcodes
//! per-OS permission copy.

use serde::{Deserialize, Serialize};

/// How to fix a missing permission: one plain sentence, then ordered steps,
/// some of which are a command to copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HowToFix {
    pub summary: String,
    pub steps: Vec<FixStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FixStep {
    pub text: String,
    /// A shell command the person can copy, when the step is one.
    pub command: Option<String>,
}

impl FixStep {
    pub fn text(text: &str) -> Self {
        Self {
            text: text.to_string(),
            command: None,
        }
    }
    pub fn command(text: &str, command: &str) -> Self {
        Self {
            text: text.to_string(),
            command: Some(command.to_string()),
        }
    }
}

/// One thing the OS must allow, and how a person fixes it when it is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PermissionItem {
    /// Stable display name, also the key of `open_permission_settings`.
    pub name: String,
    pub granted: bool,
    /// `None` when nothing needs doing.
    pub how_to_fix: Option<HowToFix>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PermissionStatus {
    /// A summary label (the single item's name, or both names).
    pub name: String,
    /// True when every item is granted.
    pub granted: bool,
    #[serde(default)]
    pub items: Vec<PermissionItem>,
}

impl PermissionStatus {
    /// Summarises `items`; an empty list means nothing needs granting.
    pub fn from_items(items: Vec<PermissionItem>) -> Self {
        let name = match items.as_slice() {
            [] => "Input access".to_string(),
            [only] => only.name.clone(),
            many => many
                .iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(" and "),
        };
        Self {
            name,
            granted: items.iter().all(|i| i.granted),
            items,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, granted: bool) -> PermissionItem {
        PermissionItem {
            name: name.into(),
            granted,
            how_to_fix: (!granted).then(|| HowToFix {
                summary: "Fix it".into(),
                steps: vec![],
            }),
        }
    }

    #[test]
    fn granted_only_when_every_item_is() {
        let all = PermissionStatus::from_items(vec![item("A", true), item("B", true)]);
        assert!(all.granted);
        assert_eq!(all.name, "A and B");
        let some = PermissionStatus::from_items(vec![item("A", true), item("B", false)]);
        assert!(!some.granted);
    }

    #[test]
    fn nothing_to_grant_is_granted() {
        assert!(PermissionStatus::from_items(vec![]).granted);
    }
}
