//! Loads the effective keymap and remap rules: the embedded defaults
//! (`config/keymaps/*.toml`) merged with the user's files in
//! [`overrides_dir`]. The same code serves every OS; only the data differs.
//!
//! A broken user file never stops the daemon: it is skipped, a warning is
//! recorded in [`Loaded::warnings`] and logged, and the defaults apply.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use flow_core::device::HostOs;
use flow_core::keymap::remap::{self, RemapRule};
use flow_core::keymap::Keymap;

const DEFAULT_REMAP_TOML: &str = include_str!("../../config/keymaps/remap.toml");

/// The OS this binary was built for.
pub const fn host_os() -> HostOs {
    if cfg!(target_os = "windows") {
        HostOs::Windows
    } else if cfg!(target_os = "macos") {
        HostOs::Macos
    } else {
        HostOs::Linux
    }
}

/// `<config dir>/keymaps`, or `$FLOW_CONFIG_DIR/keymaps` when set. Holds
/// `<os>.toml` (key overrides) and `remap.toml` (remap rules).
pub fn overrides_dir() -> Option<PathBuf> {
    let base = match std::env::var_os("FLOW_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => directories::ProjectDirs::from("dev", "Flow", "flow-daemon")?
            .config_dir()
            .to_path_buf(),
    };
    Some(base.join("keymaps"))
}

#[derive(Debug, Clone)]
pub struct Loaded {
    pub keymap: Keymap,
    pub remap_rules: Vec<RemapRule>,
    /// Problems with user files that were skipped, ready to show or log.
    pub warnings: Vec<String>,
}

fn os_name(os: HostOs) -> &'static str {
    match os {
        HostOs::Windows => "windows",
        HostOs::Macos => "macos",
        HostOs::Linux => "linux",
    }
}

/// Builds the effective keymap and rules for `os` from `dir` (`None` =
/// embedded defaults only). A missing user file is not an error.
pub fn load(os: HostOs, dir: Option<&Path>) -> Loaded {
    let mut warnings = Vec::new();
    let mut keymap = Keymap::embedded(os);
    let mut remap_rules =
        remap::parse_rules(DEFAULT_REMAP_TOML).expect("embedded remap.toml is valid");

    if let Some(dir) = dir {
        let path = dir.join(format!("{}.toml", os_name(os)));
        if let Some(text) = read_user_file(&path, &mut warnings) {
            match keymap.with_overrides(&text) {
                Ok(merged) => keymap = merged,
                Err(err) => warnings.push(skipped(&path, &err.to_string())),
            }
        }
        let path = dir.join("remap.toml");
        if let Some(text) = read_user_file(&path, &mut warnings) {
            match remap::parse_rules(&text) {
                Ok(rules) => remap_rules.extend(rules),
                Err(err) => warnings.push(skipped(&path, &err.to_string())),
            }
        }
    }
    Loaded {
        keymap,
        remap_rules,
        warnings,
    }
}

fn skipped(path: &Path, err: &str) -> String {
    format!("ignoring {}: {err}; using defaults", path.display())
}

fn read_user_file(path: &Path, warnings: &mut Vec<String>) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            warnings.push(skipped(path, &err.to_string()));
            None
        }
    }
}

/// The effective keymap and rules for this host, loaded once per process
/// (warnings are logged on first use). Unit tests get the embedded
/// defaults only, so they don't depend on the developer's own files.
pub fn load_for_host() -> &'static Loaded {
    static LOADED: OnceLock<Loaded> = OnceLock::new();
    LOADED.get_or_init(|| {
        let dir = if cfg!(test) { None } else { overrides_dir() };
        let loaded = load(host_os(), dir.as_deref());
        for warning in &loaded.warnings {
            tracing::warn!("{warning}");
        }
        loaded
    })
}

/// The keymap the capture/injection adapters translate with.
pub fn host() -> &'static Keymap {
    &load_for_host().keymap
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flow-keymap-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn without_user_files_the_defaults_apply() {
        let dir = temp_dir("none");
        let loaded = load(HostOs::Linux, Some(&dir));
        assert!(loaded.warnings.is_empty());
        assert!(loaded.remap_rules.is_empty());
        assert_eq!(loaded.keymap.code_for_name("RETURN"), Some(28));
    }

    #[test]
    fn a_user_file_overrides_entries() {
        let dir = temp_dir("override");
        fs::write(
            dir.join("linux.toml"),
            "os = \"linux\"\nkeys = [{ name = \"CAPS_LOCK\", code = 1 }]",
        )
        .unwrap();
        let loaded = load(HostOs::Linux, Some(&dir));
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.keymap.code_for_name("CAPS_LOCK"), Some(1));
        assert_eq!(loaded.keymap.code_for_name("TAB"), Some(15));
    }

    #[test]
    fn an_invalid_user_file_warns_and_falls_back() {
        let dir = temp_dir("invalid");
        fs::write(
            dir.join("linux.toml"),
            "os = \"linux\"\nkeys = [{ name = \"NOPE\", code = 1 }]",
        )
        .unwrap();
        fs::write(dir.join("remap.toml"), "this is not toml [").unwrap();
        let loaded = load(HostOs::Linux, Some(&dir));
        assert_eq!(loaded.warnings.len(), 2);
        assert!(loaded.warnings[0].contains("linux.toml") && loaded.warnings[0].contains("NOPE"));
        assert!(loaded.warnings[1].contains("remap.toml"));
        assert_eq!(loaded.keymap.code_for_name("RETURN"), Some(28));
        assert!(loaded.remap_rules.is_empty());
    }

    #[test]
    fn a_keymap_for_another_os_is_rejected_with_a_warning() {
        let dir = temp_dir("wrong-os");
        fs::write(dir.join("linux.toml"), "os = \"macos\"").unwrap();
        let loaded = load(HostOs::Linux, Some(&dir));
        assert_eq!(loaded.warnings.len(), 1);
    }

    #[test]
    fn user_remap_rules_are_loaded() {
        let dir = temp_dir("remap");
        fs::write(
            dir.join("remap.toml"),
            "[[rule]]\nfrom = \"Ctrl\"\nto = \"Meta\"\nwhen_target_os = \"macos\"\n",
        )
        .unwrap();
        let loaded = load(HostOs::Windows, Some(&dir));
        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded.remap_rules.len(), 1);
        assert_eq!(loaded.remap_rules[0].when_target_os, Some(HostOs::Macos));
    }

    #[test]
    fn every_os_loads_its_embedded_keymap_through_the_shared_loader() {
        for os in [HostOs::Windows, HostOs::Macos, HostOs::Linux] {
            let loaded = load(os, None);
            assert_eq!(loaded.keymap.os(), os);
            assert!(loaded.warnings.is_empty());
        }
    }
}
