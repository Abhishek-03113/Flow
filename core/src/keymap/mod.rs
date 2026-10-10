//! Data-driven keymaps: the per-OS table between native key codes and the
//! cross-platform names in [`crate::protocol::key_names`].
//!
//! Defaults ship in `config/keymaps/*.toml` (embedded at compile time);
//! users override entries with a file of the same shape. Parsing, merging
//! and lookup are OS-independent — the per-OS difference is data. Loading
//! files from disk lives in `flow_platform::keymap`.

pub mod remap;

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::device::HostOs;
use crate::protocol::key_names;

/// One key: the native `code` (evdev code, Windows virtual-key, macOS
/// `CGKeyCode`) and the wire `name` it is known by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyEntry {
    pub name: String,
    pub code: u32,
    /// Extra names that inject this key (never produced on capture).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// A platform-specific key with no cross-platform name (Windows' menu
    /// key, `F21`..): exempt from the vocabulary check.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local: bool,
}

/// The on-disk shape of a keymap file (default or user override).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeymapFile {
    pub os: HostOs,
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub keys: Vec<KeyEntry>,
}

fn default_version() -> u32 {
    1
}

#[derive(Debug, thiserror::Error)]
pub enum KeymapError {
    #[error("invalid TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("key name {0:?} is not in the shared vocabulary (mark it `local = true` if it is platform-specific)")]
    UnknownName(String),
    #[error("key name {0:?} is defined more than once")]
    DuplicateName(String),
    #[error("key code {code:#x} is mapped to both {first:?} and {second:?}")]
    DuplicateCode {
        code: u32,
        first: String,
        second: String,
    },
    #[error("keymap is for {found:?} but {expected:?} was expected")]
    OsMismatch { expected: HostOs, found: HostOs },
}

/// A validated keymap with both lookup directions indexed.
#[derive(Debug, Clone)]
pub struct Keymap {
    file: KeymapFile,
    by_code: HashMap<u32, usize>,
    by_name: HashMap<String, u32>,
}

impl Keymap {
    /// Validates `file` (known names, no duplicate names or codes) and
    /// indexes it.
    pub fn from_file(file: KeymapFile) -> Result<Self, KeymapError> {
        let mut by_code: HashMap<u32, usize> = HashMap::new();
        let mut by_name = HashMap::new();
        for (index, entry) in file.keys.iter().enumerate() {
            if let Some(&first) = by_code.get(&entry.code) {
                return Err(KeymapError::DuplicateCode {
                    code: entry.code,
                    first: file.keys[first].name.clone(),
                    second: entry.name.clone(),
                });
            }
            by_code.insert(entry.code, index);
            for name in std::iter::once(&entry.name).chain(&entry.aliases) {
                if !entry.local && !key_names::is_known(name) {
                    return Err(KeymapError::UnknownName(name.clone()));
                }
                if by_name.insert(name.clone(), entry.code).is_some() {
                    return Err(KeymapError::DuplicateName(name.clone()));
                }
            }
        }
        Ok(Self {
            file,
            by_code,
            by_name,
        })
    }

    pub fn parse(text: &str) -> Result<Self, KeymapError> {
        Self::from_file(toml::from_str(text)?)
    }

    /// The embedded default keymap for `os`.
    pub fn embedded(os: HostOs) -> Self {
        let text = match os {
            HostOs::Windows => include_str!("../../../config/keymaps/windows.toml"),
            HostOs::Macos => include_str!("../../../config/keymaps/macos.toml"),
            HostOs::Linux => include_str!("../../../config/keymaps/linux.toml"),
        };
        Self::parse(text).expect("embedded keymap is valid (checked by tests)")
    }

    /// Applies a user override file: its entries replace any default entry that
    /// shares their code or any of their names; everything else is kept.
    pub fn with_overrides(&self, user_toml: &str) -> Result<Self, KeymapError> {
        let user: KeymapFile = toml::from_str(user_toml)?;
        if user.os != self.file.os {
            return Err(KeymapError::OsMismatch {
                expected: self.file.os,
                found: user.os,
            });
        }
        let user_codes: HashSet<u32> = user.keys.iter().map(|k| k.code).collect();
        let user_names: HashSet<&str> = user
            .keys
            .iter()
            .flat_map(|k| std::iter::once(&k.name).chain(&k.aliases))
            .map(String::as_str)
            .collect();
        let mut keys: Vec<KeyEntry> = self
            .file
            .keys
            .iter()
            .filter(|k| !user_codes.contains(&k.code) && !user_names.contains(k.name.as_str()))
            .cloned()
            .map(|mut k| {
                k.aliases.retain(|a| !user_names.contains(a.as_str()));
                k
            })
            .collect();
        keys.extend(user.keys);
        Self::from_file(KeymapFile {
            keys,
            ..self.file.clone()
        })
    }

    pub fn os(&self) -> HostOs {
        self.file.os
    }

    /// The serializable form (what `get_keymap` returns).
    pub fn file(&self) -> &KeymapFile {
        &self.file
    }

    /// Capture direction: native code to wire name.
    pub fn name_for_code(&self, code: u32) -> Option<&str> {
        self.by_code
            .get(&code)
            .map(|&i| self.file.keys[i].name.as_str())
    }

    /// Injection direction: wire name (or alias) to native code.
    pub fn code_for_name(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_OS: [HostOs; 3] = [HostOs::Windows, HostOs::Macos, HostOs::Linux];

    #[test]
    fn embedded_keymaps_are_valid_and_for_their_os() {
        for os in ALL_OS {
            assert_eq!(Keymap::embedded(os).os(), os);
        }
    }

    #[test]
    fn embedded_keymaps_cover_the_vocabulary_except_known_gaps() {
        for os in ALL_OS {
            let keymap = Keymap::embedded(os);
            for name in key_names::all() {
                let missing = keymap.code_for_name(&name).is_none();
                // Windows has no `Fn` key.
                let expected_gap = os == HostOs::Windows && name == key_names::FUNCTION;
                assert_eq!(missing, expected_gap, "{os:?} {name}");
            }
        }
    }

    #[test]
    fn every_embedded_entry_round_trips_both_directions() {
        for os in ALL_OS {
            let keymap = Keymap::embedded(os);
            for entry in &keymap.file().keys {
                assert_eq!(keymap.name_for_code(entry.code), Some(entry.name.as_str()));
                assert_eq!(keymap.code_for_name(&entry.name), Some(entry.code));
            }
        }
    }

    #[test]
    fn spot_checks_against_the_native_codes() {
        let win = Keymap::embedded(HostOs::Windows);
        assert_eq!(win.code_for_name("RETURN"), Some(0x0D));
        assert_eq!(win.name_for_code(0xA0), Some("SHIFT"));
        assert_eq!(win.code_for_name("F24"), Some(0x87));
        let mac = Keymap::embedded(HostOs::Macos);
        assert_eq!(mac.code_for_name("A"), Some(0x00));
        assert_eq!(mac.name_for_code(0x37), Some("COMMAND"));
        let linux = Keymap::embedded(HostOs::Linux);
        assert_eq!(linux.code_for_name("RETURN"), Some(28));
        assert_eq!(linux.name_for_code(30), Some("A"));
    }

    #[test]
    fn rejects_a_name_outside_the_vocabulary() {
        let err =
            Keymap::parse("os = \"linux\"\nkeys = [{ name = \"NOPE\", code = 1 }]").unwrap_err();
        assert!(matches!(err, KeymapError::UnknownName(n) if n == "NOPE"));
    }

    #[test]
    fn local_keys_may_use_their_own_names() {
        let keymap =
            Keymap::parse("os = \"linux\"\nkeys = [{ name = \"APPS\", code = 1, local = true }]")
                .unwrap();
        assert_eq!(keymap.code_for_name("APPS"), Some(1));
    }

    #[test]
    fn rejects_duplicate_codes_and_names() {
        let dup_code =
            "os = \"linux\"\nkeys = [{ name = \"A\", code = 1 }, { name = \"B\", code = 1 }]";
        assert!(matches!(
            Keymap::parse(dup_code).unwrap_err(),
            KeymapError::DuplicateCode { code: 1, .. }
        ));
        let dup_name =
            "os = \"linux\"\nkeys = [{ name = \"A\", code = 1 }, { name = \"A\", code = 2 }]";
        assert!(matches!(
            Keymap::parse(dup_name).unwrap_err(),
            KeymapError::DuplicateName(n) if n == "A"
        ));
    }

    #[test]
    fn aliases_inject_but_never_capture() {
        let keymap = Keymap::parse(
            "os = \"linux\"\nkeys = [{ name = \"RETURN\", code = 28, aliases = [\"SPACE\"] }]",
        )
        .unwrap();
        assert_eq!(keymap.code_for_name("SPACE"), Some(28));
        assert_eq!(keymap.name_for_code(28), Some("RETURN"));
    }

    #[test]
    fn an_alias_colliding_with_a_name_is_rejected() {
        let text = "os = \"linux\"\nkeys = [{ name = \"A\", code = 1, aliases = [\"B\"] }, { name = \"B\", code = 2 }]";
        assert!(matches!(
            Keymap::parse(text).unwrap_err(),
            KeymapError::DuplicateName(_)
        ));
    }

    #[test]
    fn overrides_replace_by_name_or_code_and_keep_the_rest() {
        let base = Keymap::embedded(HostOs::Linux);
        let merged = base
            .with_overrides(
                "os = \"linux\"\nkeys = [{ name = \"CAPS_LOCK\", code = 1 }, { name = \"TAB\", code = 30 }]",
            )
            .unwrap();
        // CAPS_LOCK moved onto code 1, displacing ESCAPE which sat there.
        assert_eq!(merged.code_for_name("CAPS_LOCK"), Some(1));
        assert_eq!(merged.name_for_code(1), Some("CAPS_LOCK"));
        assert_eq!(merged.code_for_name("ESCAPE"), None);
        // TAB took A's code, so old TAB (15) and A (30) are both gone.
        assert_eq!(merged.name_for_code(30), Some("TAB"));
        assert_eq!(merged.name_for_code(15), None);
        assert_eq!(merged.code_for_name("A"), None);
        // Untouched entries survive.
        assert_eq!(merged.code_for_name("B"), base.code_for_name("B"));
    }

    #[test]
    fn overrides_for_another_os_are_rejected() {
        let err = Keymap::embedded(HostOs::Linux)
            .with_overrides("os = \"macos\"")
            .unwrap_err();
        assert!(matches!(err, KeymapError::OsMismatch { .. }));
    }

    #[test]
    fn keymap_file_round_trips_through_json_for_the_control_api() {
        let file = Keymap::embedded(HostOs::Windows).file().clone();
        let json = serde_json::to_string(&file).unwrap();
        assert_eq!(serde_json::from_str::<KeymapFile>(&json).unwrap(), file);
    }
}
