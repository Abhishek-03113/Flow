//! The scenario data model: what `harness/scenarios/*.toml` may say, and
//! how a scenario expands across the OS matrix. See `docs/testing/harness.md`
//! for the reference.

use std::fmt;
use std::path::{Path, PathBuf};

use flow_core::control::Role;
use flow_core::device::HostOs;
use serde::Deserialize;

use crate::events::{os_label, Kind};

/// An OS assignment for the slots `$a`, `$b`, `$c` of one matrix cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combo {
    /// `win-mac` or `win-mac-lin`.
    pub label: String,
    pub oses: Vec<HostOs>,
}

pub const ALL_OS: [HostOs; 3] = [HostOs::Windows, HostOs::Macos, HostOs::Linux];

pub fn os_short(os: HostOs) -> &'static str {
    match os {
        HostOs::Windows => "win",
        HostOs::Macos => "mac",
        HostOs::Linux => "lin",
    }
}

pub fn parse_os_short(text: &str) -> Option<HostOs> {
    ALL_OS
        .into_iter()
        .find(|os| os_short(*os) == text || os_label(*os) == text)
}

impl Combo {
    pub fn new(oses: Vec<HostOs>) -> Self {
        let label = oses
            .iter()
            .map(|os| os_short(*os))
            .collect::<Vec<_>>()
            .join("-");
        Self { label, oses }
    }

    /// All 9 controller x controlled pairs.
    pub fn all_pairs() -> Vec<Combo> {
        ALL_OS
            .into_iter()
            .flat_map(|a| ALL_OS.into_iter().map(move |b| Combo::new(vec![a, b])))
            .collect()
    }

    /// A representative set of triples: every OS leads once and every OS
    /// appears in every position at least once.
    pub fn representative_triples() -> Vec<Combo> {
        use HostOs::{Linux, Macos, Windows};
        [
            [Windows, Macos, Linux],
            [Macos, Linux, Windows],
            [Linux, Windows, Macos],
            [Windows, Windows, Macos],
        ]
        .into_iter()
        .map(|oses| Combo::new(oses.to_vec()))
        .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Matrix {
    /// controller x controlled: `$a` is the first device's OS, `$b` the second's.
    #[default]
    Pairs,
    Triples,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceDecl {
    pub name: String,
    /// `windows` | `macos` | `linux`, or a slot: `$a`, `$b`, `$c`.
    pub os: String,
    pub role: Role,
    /// `[width, height]`; default 1920x1080.
    pub screen: Option<[u32; 2]>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkDecl {
    /// The side that initiates pairing.
    pub a: String,
    pub b: String,
    /// `live` or `refused:<reason>`; default: live, or `refused:role_conflict`
    /// when both devices have the same role.
    pub expect: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldDecl {
    /// `secure` (default; real Noise sessions and pairing consent) or `insecure`.
    pub security: Option<String>,
    /// Hotkey debounce in ms; default 0 so scenarios can switch quickly.
    pub debounce_ms: Option<u64>,
    /// Deadline for every wait in the scenario, in ms.
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub matrix: Matrix,
    /// Restrict the matrix to these cells, e.g. `["win-mac"]`.
    pub pairs: Option<Vec<String>>,
    #[serde(default)]
    pub world: WorldDecl,
    /// Empty = a controller `ctl` (`$a`) and a controlled `ctd` (`$b`).
    #[serde(default)]
    pub devices: Vec<DeviceDecl>,
    /// Empty = every controller linked to every controlled device, in order.
    #[serde(default)]
    pub links: Vec<LinkDecl>,
    pub steps: Vec<Step>,
}

impl Scenario {
    pub fn combos(&self) -> Result<Vec<Combo>, String> {
        let all = match self.matrix {
            Matrix::Pairs => Combo::all_pairs(),
            Matrix::Triples => Combo::representative_triples(),
        };
        let Some(only) = &self.pairs else {
            return Ok(all);
        };
        only.iter()
            .map(|label| {
                all.iter()
                    .find(|c| &c.label == label)
                    .cloned()
                    .ok_or_else(|| format!("{}: unknown matrix cell {label:?}", self.name))
            })
            .collect()
    }
}

/// One step: the common `when` guard plus what to do.
#[derive(Debug, Clone)]
pub struct Step {
    /// Run only when the condition holds, e.g. `ctd=macos` or `ctd!=macos`.
    pub when: Option<String>,
    pub kind: StepKind,
}

impl<'de> Deserialize<'de> for Step {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let mut table = toml::Table::deserialize(deserializer)?;
        let when = match table.remove("when") {
            None => None,
            Some(toml::Value::String(text)) => Some(text),
            Some(_) => return Err(D::Error::custom("`when` must be a string")),
        };
        let kind = StepKind::deserialize(toml::Value::Table(table)).map_err(D::Error::custom)?;
        Ok(Step { when, kind })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case", deny_unknown_fields)]
pub enum StepKind {
    /// Physically press a key (by its wire name) on a device.
    Press {
        on: String,
        key: String,
    },
    Release {
        on: String,
        key: String,
    },
    /// Press and release each character (letters, digits, space).
    Type {
        on: String,
        text: String,
    },
    /// Move the physical pointer by (dx, dy), `times` times.
    Move {
        on: String,
        dx: i32,
        dy: i32,
        #[serde(default = "one")]
        times: u32,
    },
    ButtonDown {
        on: String,
        #[serde(default = "left")]
        button: String,
    },
    ButtonUp {
        on: String,
        #[serde(default = "left")]
        button: String,
    },
    /// Button down then up.
    Click {
        on: String,
        #[serde(default = "left")]
        button: String,
    },
    Scroll {
        on: String,
        dx: i32,
        dy: i32,
    },
    /// Put the device's own pointer at (x, y) (it watches this for edge switching).
    PlaceCursor {
        on: String,
        x: i32,
        y: i32,
    },
    /// Press the switch key chord, or with `which = "return_home"` the return-home chord.
    Hotkey {
        on: String,
        #[serde(default)]
        which: Hotkey,
    },
    /// The control API: `switch_to` on a controller.
    SwitchTo {
        on: String,
        target: String,
        /// Expect the command to be refused with this error code.
        expect_error: Option<String>,
    },
    /// The control API: `release_control` on a controlled device.
    ReleaseRequest {
        on: String,
        expect_error: Option<String>,
    },
    /// Kill a device's engine abruptly.
    Disconnect {
        device: String,
    },
    /// Restart it on its old data and wait for its links.
    Reconnect {
        device: String,
    },
    /// `update_settings` with one field, e.g. `key = "share_mouse", value = false`.
    SetSetting {
        on: String,
        key: String,
        value: toml::Value,
    },
    SetRemaps {
        on: String,
        rules: Vec<RemapDecl>,
    },
    /// Replace the screen arrangement; `device = "local"` is the controller itself.
    SetLayout {
        on: String,
        screens: Vec<ScreenDecl>,
    },
    /// Start a fresh observation window (forget recorded input).
    Clear {
        on: Option<String>,
    },
    /// Repeat nested steps.
    Repeat {
        times: u32,
        steps: Vec<Step>,
    },
    /// Wait (polling, with a deadline) until the check holds.
    Wait(Check),
    /// Wait for the world to go quiet, then require the check to hold.
    Expect(Check),
}

fn one() -> u32 {
    1
}

fn left() -> String {
    "left".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hotkey {
    #[default]
    Switch,
    ReturnHome,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemapDecl {
    pub from: String,
    pub to: String,
    pub when_target_os: Option<HostOs>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenDecl {
    pub device: String,
    pub x: i32,
    pub y: i32,
}

/// Something to verify about the world.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "that", rename_all = "snake_case", deny_unknown_fields)]
pub enum Check {
    /// The controller's active screen: `local` or a device name.
    Active {
        on: String,
        is: String,
    },
    /// Exactly these events were injected into `on` since the last clear,
    /// compared by that OS's native key codes.
    Injected {
        on: String,
        events: Vec<String>,
        /// Repeat the list this many times.
        #[serde(default = "one")]
        repeat: u32,
        /// Look only at these event kinds.
        #[serde(default)]
        kinds: Vec<Kind>,
    },
    /// These events appear consecutively, in order, among those injected.
    InjectedIncludes {
        on: String,
        events: Vec<String>,
        #[serde(default)]
        kinds: Vec<Kind>,
    },
    /// The summed injected pointer motion, within `tolerance` per axis.
    PointerMoved {
        on: String,
        dx: i32,
        dy: i32,
        #[serde(default)]
        tolerance: i32,
    },
    /// Exactly these events reached the device's own applications.
    Delivered {
        on: String,
        events: Vec<String>,
        #[serde(default = "one")]
        repeat: u32,
        #[serde(default)]
        kinds: Vec<Kind>,
    },
    /// Nothing (of these kinds, default all) reached the device's own applications.
    NoLocalLeak {
        on: String,
        #[serde(default)]
        kinds: Vec<Kind>,
    },
    /// No physical input fed to `on` reached any other device.
    NoForwarding {
        on: String,
    },
    /// This key never reached any device's applications (neither injected
    /// nor delivered locally), e.g. the last key of the hotkey chord.
    KeyNeverSeen {
        key: String,
    },
    /// Every injected key and button press was released again.
    NothingStuck {
        on: String,
    },
    Suppressed {
        on: String,
        is: bool,
    },
    Capturing {
        on: String,
        is: bool,
    },
    /// Controlled side: the controller is driving this machine.
    Entered {
        on: String,
        is: bool,
    },
    /// Whether `on` has a live link to `peer`.
    Linked {
        on: String,
        peer: String,
        is: bool,
    },
    /// `on`'s `link_error` for `peer`: a reason, or `none`.
    LinkError {
        on: String,
        peer: String,
        is: String,
    },
    /// The last cursor warp on `on`.
    Warped {
        on: String,
        x: i32,
        y: i32,
    },
}

impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// Reads one scenario file.
pub fn load_file(path: &Path) -> Result<Scenario, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The bundled catalogue, `harness/scenarios/*.toml`, sorted by file name.
pub fn catalogue_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios")
}

/// Loads every scenario in `dir` whose name matches `glob` (`*` and `?`).
pub fn load_dir(dir: &Path, glob: Option<&str>) -> Result<Vec<Scenario>, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    let mut scenarios = Vec::new();
    for path in paths {
        let scenario = load_file(&path)?;
        if glob.is_none_or(|g| glob_match(g, &scenario.name)) {
            scenarios.push(scenario);
        }
    }
    Ok(scenarios)
}

/// `*` matches any run of characters, `?` any one.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('*', rest)) => (0..=t.len()).any(|skip| go(rest, &t[skip..])),
            Some(('?', rest)) => !t.is_empty() && go(rest, &t[1..]),
            Some((c, rest)) => t.first() == Some(c) && go(rest, &t[1..]),
        }
    }
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    go(&p, &t)
}

/// Parses `--pairs`: `all` or comma-separated cell labels.
pub fn parse_pairs_filter(text: &str) -> Option<Vec<String>> {
    (text != "all").then(|| text.split(',').map(|s| s.trim().to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_names() {
        assert!(glob_match("basic*", "basic-switch"));
        assert!(glob_match("*switch", "basic-switch"));
        assert!(glob_match("b?sic*", "basic-switch"));
        assert!(!glob_match("basic", "basic-switch"));
    }

    #[test]
    fn nine_pairs_with_stable_labels() {
        let pairs = Combo::all_pairs();
        assert_eq!(pairs.len(), 9);
        assert_eq!(pairs[1].label, "win-mac");
    }

    #[test]
    fn a_step_reads_its_guard_and_kind() {
        let scenario: Scenario = toml::from_str(
            r#"
            name = "x"
            description = "y"
            [[steps]]
            do = "press"
            on = "ctl"
            key = "A"
            when = "ctd=macos"
            [[steps]]
            do = "expect"
            that = "active"
            on = "ctl"
            is = "local"
            "#,
        )
        .unwrap();
        assert_eq!(scenario.steps.len(), 2);
        assert_eq!(scenario.steps[0].when.as_deref(), Some("ctd=macos"));
        assert!(matches!(
            scenario.steps[1].kind,
            StepKind::Expect(Check::Active { .. })
        ));
    }

    #[test]
    fn a_typo_in_a_step_is_an_error() {
        let result: Result<Scenario, _> = toml::from_str(
            r#"
            name = "x"
            description = "y"
            [[steps]]
            do = "press"
            on = "ctl"
            kye = "A"
            "#,
        );
        assert!(result.is_err());
    }
}
