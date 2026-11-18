//! What the OS must allow before Flow can share input, probed for real.
//!
//! * macOS: Accessibility (post events) and Input Monitoring (listen to
//!   events) are separate grants; each can be requested to show the OS
//!   prompt. macOS applies Input Monitoring to a running app only after it
//!   restarts, so a grant made while Flow runs asks for a restart.
//! * Windows: no input permission, but Windows Defender Firewall blocks
//!   other computers from reaching Flow until it allows UDP 47824 and TCP
//!   47825 (and dismissing its first prompt writes a *block* rule).
//!   [`request`] adds the allow rules, and removes Flow's inbound block
//!   rules, behind one UAC prompt.
//! * Linux: read access to `/dev/input/event*`, write access to
//!   `/dev/uinput` and, when a firewall (ufw or firewalld) is active, its
//!   permission for Flow's two ports. [`request`] fixes all of it at once
//!   with one administrator prompt (`pkexec`): a udev rule that gives the
//!   signed-in user access right away (`uaccess`, no log out needed), the
//!   `input` group as a fallback, the `uinput` module and the firewall
//!   rules.
//!
//! The macOS and Windows branches are compile-checked (cross clippy) but
//! have not run on real hardware; they are kept deliberately small.

use flow_core::permission::PermissionItem;
use flow_core::permission::{FixStep, HowToFix};

pub const ACCESSIBILITY: &str = "Accessibility";
pub const INPUT_MONITORING: &str = "Input Monitoring";
pub const READ_INPUT: &str = "Read keyboard and mouse";
pub const CREATE_INPUT: &str = "Create virtual input device";
pub const INCOMING_CONNECTIONS: &str = "Incoming connections";
pub const LOCAL_NETWORK: &str = "Local network";

/// The item the engine adds when every discovery announce has kept failing
/// to leave this computer: macOS Local Network privacy denied, or no
/// network at all. Not probed here; the engine sees it from send results.
pub fn local_network_item() -> PermissionItem {
    let steps = if cfg!(target_os = "macos") {
        vec![
            FixStep::text(
                "Open System Settings \u{203a} Privacy & Security \u{203a} Local Network.",
            ),
            FixStep::text("Turn on Flow."),
            FixStep::text("Check that this Mac is connected to Wi-Fi or Ethernet."),
        ]
    } else {
        vec![
            FixStep::text("Check that this computer is connected to Wi-Fi or Ethernet."),
            FixStep::text("If a VPN or security app is running, allow Flow on the local network."),
        ]
    };
    item(LOCAL_NETWORK, false, || {
        HowToFix {
        summary: "Flow can\u{2019}t send anything to your local network, so other computers can\u{2019}t find this one.".into(),
        steps,
    }
    })
}

/// Why [`request`] could not fix what is missing. Each variant is a
/// distinct, explainable reason; the text is for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupError {
    /// The person dismissed the administrator prompt.
    Cancelled,
    /// This system cannot run the automatic setup (no `pkexec`, or no
    /// authentication agent to ask for the password).
    Unavailable(String),
    /// The setup ran and a step failed.
    Failed(String),
}

impl std::fmt::Display for SetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SetupError::Cancelled => f.write_str("the administrator prompt was dismissed"),
            SetupError::Unavailable(why) => write!(f, "automatic setup is unavailable: {why}"),
            SetupError::Failed(why) => write!(f, "setup failed: {why}"),
        }
    }
}

fn item(name: &str, granted: bool, how_to_fix: impl FnOnce() -> HowToFix) -> PermissionItem {
    PermissionItem {
        name: name.to_string(),
        granted,
        how_to_fix: (!granted).then(how_to_fix),
    }
}

/// Probes every permission the OS requires. Cheap enough to poll.
pub fn check() -> Vec<PermissionItem> {
    imp::check()
}

/// Gets everything still missing granted, the best way this OS allows:
/// macOS shows its prompts, Linux runs the one-time system setup behind a
/// single administrator prompt (blocking until the person answers). `name`
/// limits a macOS prompt to one item; `None` asks for everything missing.
pub fn request(name: Option<&str>) -> Result<(), SetupError> {
    imp::request(name)
}

/// Whether [`request`] can fix every missing item by itself (Linux with
/// `pkexec`), so the UI offers one button instead of manual steps.
pub fn one_click_setup() -> bool {
    imp::one_click_setup()
}

/// Whether this OS applies a permission granted while Flow runs only after
/// Flow restarts (macOS).
pub fn grant_needs_restart() -> bool {
    cfg!(target_os = "macos")
}

/// The OS settings pane (or `None`) where the user can grant `name`.
pub fn settings_url(name: &str) -> Option<&'static str> {
    imp::settings_url(name)
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use flow_core::channel::{DISCOVERY_PORT, PEER_PORT};

    /// Written by the setup script; lists what it configured.
    const SETUP_MARKER: &str = "/etc/flow/system-setup";
    const UDEV_RULE: &str = "/etc/udev/rules.d/70-flow.rules";

    /// The one-time system setup, run as root by `pkexec` (or by hand with
    /// `sudo sh`). Idempotent. Every step that fails says which one it was.
    /// `uaccess` (applied by systemd-logind) gives the signed-in user access
    /// immediately; the `input` group covers sessions without logind and
    /// takes effect at the next login.
    pub(super) fn setup_script() -> String {
        format!(
            r#"#!/bin/sh
# Flow one-time system setup. Safe to run more than once.
set -u
fail() {{ echo "flow-setup: $*" >&2; exit 1; }}
user=""
if [ -n "${{PKEXEC_UID:-}}" ]; then user=$(getent passwd "$PKEXEC_UID" | cut -d: -f1); fi
if [ -z "$user" ] && [ -n "${{SUDO_USER:-}}" ]; then user="$SUDO_USER"; fi
[ -n "$user" ] || fail "could not tell which account to set up (run it with sudo or pkexec)"
[ "$(id -u)" -eq 0 ] || fail "needs to run as root"

mkdir -p /etc/udev/rules.d || fail "could not create /etc/udev/rules.d"
cat > {UDEV_RULE} <<'RULES' || fail "could not write {UDEV_RULE}"
# Flow: let the signed-in user read keyboards and mice and create the
# virtual keyboard and mouse that types for the other computer.
KERNEL=="uinput", SUBSYSTEM=="misc", OPTIONS+="static_node=uinput", TAG+="uaccess", GROUP="input", MODE="0660"
SUBSYSTEM=="input", KERNEL=="event*", TAG+="uaccess"
RULES
mkdir -p /etc/modules-load.d && echo uinput > /etc/modules-load.d/flow-uinput.conf \
  || fail "could not write /etc/modules-load.d/flow-uinput.conf"
modprobe uinput 2>/dev/null || [ -e /dev/uinput ] || fail "the uinput kernel module is not available"
getent group input >/dev/null || groupadd --system input || fail "could not create the input group"
usermod -aG input "$user" || fail "could not add $user to the input group"
command -v udevadm >/dev/null 2>&1 || fail "udevadm was not found"
udevadm control --reload-rules || fail "udev did not reload its rules"
udevadm trigger --action=change --subsystem-match=input --subsystem-match=misc || fail "udev did not apply the rules"
udevadm settle --timeout=10 2>/dev/null || true

firewalls=""
if command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | grep -q "Status: active"; then
  ufw allow {DISCOVERY_PORT}/udp comment 'Flow discovery' >/dev/null || fail "ufw refused to allow UDP {DISCOVERY_PORT}"
  ufw allow {PEER_PORT}/tcp comment 'Flow connections' >/dev/null || fail "ufw refused to allow TCP {PEER_PORT}"
  firewalls="$firewalls ufw"
fi
if command -v firewall-cmd >/dev/null 2>&1 && firewall-cmd --state >/dev/null 2>&1; then
  firewall-cmd --permanent --add-port={DISCOVERY_PORT}/udp --add-port={PEER_PORT}/tcp >/dev/null \
    || fail "firewalld refused to open UDP {DISCOVERY_PORT} and TCP {PEER_PORT}"
  firewall-cmd --reload >/dev/null || fail "firewalld did not reload"
  firewalls="$firewalls firewalld"
fi

mkdir -p /etc/flow && printf 'version=1\nuser=%s\nfirewalls=%s\n' "$user" "$firewalls" > {SETUP_MARKER} \
  || fail "could not write {SETUP_MARKER}"
echo "flow-setup: done for $user (firewalls:${{firewalls:- none}})"
"#
        )
    }

    /// The setup script saved where a person can run it by hand, for when
    /// the one-click path is unavailable. `None` if it cannot be written.
    fn manual_script_path() -> Option<PathBuf> {
        let dir = directories::ProjectDirs::from("dev", "Flow", "flow-daemon")?
            .data_dir()
            .to_path_buf();
        let path = dir.join("flow-linux-setup.sh");
        let script = setup_script();
        if std::fs::read_to_string(&path).ok().as_deref() != Some(script.as_str()) {
            std::fs::create_dir_all(&dir).ok()?;
            std::fs::write(&path, script).ok()?;
        }
        Some(path)
    }

    fn setup_marker() -> Option<String> {
        std::fs::read_to_string(SETUP_MARKER).ok()
    }

    /// Fix text shared by every Linux item: one button in the app, or one
    /// command by hand; and, when the setup already ran but this session
    /// still has no access, the precise reason.
    fn fix(summary: &str) -> HowToFix {
        if setup_marker().is_some() && Path::new(UDEV_RULE).exists() {
            return HowToFix {
                summary: "Flow\u{2019}s system setup is installed, but this session has not picked it up yet.".into(),
                steps: vec![FixStep::text(
                    "Log out and back in (or restart the computer), then open Flow again.",
                )],
            };
        }
        let one_click = find_pkexec().is_some();
        let mut steps = Vec::new();
        if one_click {
            steps.push(FixStep::text(
                "Click \u{201c}Set Up Automatically\u{201d} and enter your password once. Flow sets up everything it needs.",
            ));
        }
        if let Some(path) = manual_script_path() {
            steps.push(FixStep::command(
                if one_click {
                    "Or run this in a terminal:"
                } else {
                    "Run this in a terminal and enter your password:"
                },
                &format!("sudo sh '{}'", path.display()),
            ));
        }
        HowToFix {
            summary: summary.into(),
            steps,
        }
    }

    fn can_read_an_input_device() -> bool {
        let Ok(entries) = std::fs::read_dir("/dev/input") else {
            return false;
        };
        entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("event"))
            .any(|e| OpenOptions::new().read(true).open(e.path()).is_ok())
    }

    fn ufw_enabled() -> bool {
        std::fs::read_to_string("/etc/ufw/ufw.conf").is_ok_and(|conf| {
            conf.lines()
                .any(|line| line.trim().eq_ignore_ascii_case("ENABLED=yes"))
        })
    }

    fn firewalld_running() -> bool {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return false;
        };
        entries.flatten().any(|e| {
            e.file_name()
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_digit())
                && std::fs::read_to_string(e.path().join("comm"))
                    .is_ok_and(|comm| comm.trim() == "firewalld")
        })
    }

    /// The firewalls that are on, and those of them Flow's setup has not
    /// opened its ports in.
    fn firewalls() -> (Vec<&'static str>, Vec<&'static str>) {
        let active: Vec<&'static str> =
            [("ufw", ufw_enabled()), ("firewalld", firewalld_running())]
                .into_iter()
                .filter(|(_, on)| *on)
                .map(|(name, _)| name)
                .collect();
        let configured = setup_marker()
            .and_then(|marker| {
                marker
                    .lines()
                    .find_map(|l| l.strip_prefix("firewalls=").map(str::to_string))
            })
            .unwrap_or_default();
        let blocking = active
            .iter()
            .copied()
            .filter(|name| !configured.split_whitespace().any(|c| c == *name))
            .collect();
        (active, blocking)
    }

    pub fn check() -> Vec<PermissionItem> {
        let mut items = vec![
            item(READ_INPUT, can_read_an_input_device(), || {
                fix("Flow needs permission to read your keyboard and mouse.")
            }),
            item(
                CREATE_INPUT,
                OpenOptions::new().write(true).open("/dev/uinput").is_ok(),
                || fix("Flow needs permission to create a virtual keyboard and mouse."),
            ),
        ];
        // Only relevant when a firewall is on: otherwise nothing blocks
        // other computers from reaching Flow.
        let (active, blocking) = firewalls();
        if !active.is_empty() {
            items.push(item(INCOMING_CONNECTIONS, blocking.is_empty(), || {
                let mut how = fix(&format!(
                    "Your firewall ({}) blocks other computers from finding and connecting to this one.",
                    blocking.join(", ")
                ));
                how.steps.push(FixStep::text(&format!(
                    "Flow uses UDP port {DISCOVERY_PORT} and TCP port {PEER_PORT}."
                )));
                how
            }));
        }
        items
    }

    fn find_pkexec() -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .chain(["/usr/bin".into(), "/bin".into()])
            .map(|dir| dir.join("pkexec"))
            .find(|candidate| candidate.is_file())
    }

    pub fn one_click_setup() -> bool {
        find_pkexec().is_some()
    }

    pub fn request(_name: Option<&str>) -> Result<(), SetupError> {
        if check().iter().all(|i| i.granted) {
            return Ok(());
        }
        let pkexec = find_pkexec()
            .ok_or_else(|| SetupError::Unavailable("pkexec (polkit) is not installed".into()))?;
        tracing::info!(target: "flow", "[SETUP] asking for administrator access to set up input devices and the firewall");
        let mut child = Command::new(pkexec)
            .args(["/bin/sh", "-s"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| SetupError::Unavailable(format!("could not start pkexec: {err}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(setup_script().as_bytes()).map_err(|err| {
                SetupError::Failed(format!("could not pass the setup script to pkexec: {err}"))
            })?;
        }
        let output = child
            .wait_with_output()
            .map_err(|err| SetupError::Failed(format!("pkexec did not finish: {err}")))?;
        // The script prefixes its own messages for the log; the reason is
        // what follows.
        let stderr = String::from_utf8_lossy(&output.stderr)
            .lines()
            .map(|line| line.trim_start_matches("flow-setup: "))
            .collect::<Vec<_>>()
            .join("; ")
            .trim()
            .to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        match output.status.code() {
            Some(0) => {
                tracing::info!(target: "flow", "[SETUP] {stdout}");
                Ok(())
            }
            // pkexec: 126 = the dialog was dismissed / not authorised.
            Some(126) => Err(SetupError::Cancelled),
            // pkexec: 127 = authentication could not happen (no agent,
            // wrong password three times).
            Some(127) => Err(SetupError::Unavailable(if stderr.is_empty() {
                "pkexec could not authenticate (no polkit authentication agent is running?)".into()
            } else {
                stderr
            })),
            code => Err(SetupError::Failed(if stderr.is_empty() {
                format!("the setup script exited with {code:?}")
            } else {
                stderr
            })),
        }
    }

    pub fn settings_url(_name: &str) -> Option<&'static str> {
        None
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_setup_script_opens_both_flow_ports_and_installs_the_rule() {
            let script = setup_script();
            assert!(script.contains(&format!("{DISCOVERY_PORT}/udp")));
            assert!(script.contains(&format!("{PEER_PORT}/tcp")));
            assert!(script.contains("TAG+=\"uaccess\""));
            assert!(script.contains(SETUP_MARKER));
        }

        /// The script must at least parse; `sh -n` checks syntax without
        /// running anything.
        #[test]
        fn the_setup_script_is_valid_shell() {
            let mut child = Command::new("/bin/sh")
                .arg("-n")
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("sh");
            child
                .stdin
                .take()
                .unwrap()
                .write_all(setup_script().as_bytes())
                .unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

/// PowerShell for the Windows firewall item, kept free of Windows APIs so
/// it is unit-tested on every OS.
#[cfg(any(target_os = "windows", test))]
mod windows_firewall {
    use flow_core::channel::{DISCOVERY_PORT, PEER_PORT};

    /// The rule group Flow's allow rules live in (also their display names'
    /// prefix), so setup can replace them idempotently.
    pub const GROUP: &str = "Flow";

    /// What the probe found. The probe script's exit codes map onto it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum State {
        /// Firewall off, or both allow rules present and no block rule.
        Open,
        /// The allow rules are missing.
        Missing,
        /// An inbound block rule for Flow's program exists (the first
        /// "Allow access" prompt was dismissed).
        Blocked,
    }

    pub fn state_from_exit(code: Option<i32>) -> Option<State> {
        match code {
            Some(0) => Some(State::Open),
            Some(1) => Some(State::Missing),
            Some(2) => Some(State::Blocked),
            _ => None,
        }
    }

    fn quoted(text: &str) -> String {
        format!("'{}'", text.replace('\'', "''"))
    }

    /// Exits 0 (open), 1 (missing) or 2 (blocked). Reads only; needs no
    /// administrator rights. Compares enum names, so the result does not
    /// depend on the Windows display language.
    pub fn probe_script(exe: &str) -> String {
        format!(
            "$ErrorActionPreference = 'SilentlyContinue'\n\
             if (-not (Get-NetFirewallProfile | Where-Object {{ $_.Enabled -eq 'True' }})) {{ exit 0 }}\n\
             $block = Get-NetFirewallApplicationFilter -Program {exe} | Get-NetFirewallRule | \
             Where-Object {{ $_.Enabled -eq 'True' -and $_.Action -eq 'Block' -and $_.Direction -eq 'Inbound' }}\n\
             if ($block) {{ exit 2 }}\n\
             $allow = @(Get-NetFirewallRule -Group {group} | \
             Where-Object {{ $_.Enabled -eq 'True' -and $_.Action -eq 'Allow' -and $_.Direction -eq 'Inbound' }})\n\
             if ($allow.Count -ge 2) {{ exit 0 }}\n\
             exit 1\n",
            exe = quoted(exe),
            group = quoted(GROUP),
        )
    }

    /// Run elevated: removes Flow's inbound block rules and its old allow
    /// rules, then allows UDP 47824 and TCP 47825 on every profile.
    pub fn setup_script(exe: &str) -> String {
        format!(
            "$ErrorActionPreference = 'Stop'\n\
             Get-NetFirewallApplicationFilter -Program {exe} -ErrorAction SilentlyContinue | Get-NetFirewallRule | \
             Where-Object {{ $_.Action -eq 'Block' -and $_.Direction -eq 'Inbound' }} | Remove-NetFirewallRule\n\
             Get-NetFirewallRule -Group {group} -ErrorAction SilentlyContinue | Remove-NetFirewallRule\n\
             New-NetFirewallRule -DisplayName 'Flow (UDP {DISCOVERY_PORT})' -Group {group} -Direction Inbound \
             -Action Allow -Protocol UDP -LocalPort {DISCOVERY_PORT} -Profile Any | Out-Null\n\
             New-NetFirewallRule -DisplayName 'Flow (TCP {PEER_PORT})' -Group {group} -Direction Inbound \
             -Action Allow -Protocol TCP -LocalPort {PEER_PORT} -Profile Any | Out-Null\n\
             exit 0\n",
            exe = quoted(exe),
            group = quoted(GROUP),
        )
    }

    /// `ERROR_CANCELLED`: the outer script's exit code when the UAC prompt
    /// was dismissed.
    pub const CANCELLED: i32 = 1223;

    /// Runs `inner` elevated (one UAC prompt) and exits with its exit code,
    /// or [`CANCELLED`] when the prompt was dismissed.
    pub fn elevate_script(inner: &str) -> String {
        format!(
            "try {{ $p = Start-Process -FilePath powershell.exe -Verb RunAs -Wait -PassThru -WindowStyle Hidden \
             -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{}' }} catch {{ exit {CANCELLED} }}\n\
             exit $p.ExitCode\n",
            encode(inner)
        )
    }

    /// `-EncodedCommand` form: base64 of the UTF-16LE text. No quoting
    /// survives being nested through `Start-Process`, but base64 has none.
    pub fn encode(script: &str) -> String {
        let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        base64(&bytes)
    }

    fn base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let n = (u32::from(chunk[0]) << 16)
                | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
                | u32::from(*chunk.get(2).unwrap_or(&0));
            for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
                if i <= chunk.len() {
                    out.push(char::from(ALPHABET[(n >> shift) as usize & 63]));
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn base64_matches_known_vectors() {
            assert_eq!(base64(b""), "");
            assert_eq!(base64(b"f"), "Zg==");
            assert_eq!(base64(b"fo"), "Zm8=");
            assert_eq!(base64(b"foo"), "Zm9v");
            assert_eq!(base64(b"foobar"), "Zm9vYmFy");
            // `powershell -EncodedCommand` of "exit 0" as UTF-16LE.
            assert_eq!(encode("exit 0"), "ZQB4AGkAdAAgADAA");
        }

        #[test]
        fn scripts_open_both_ports_and_quote_the_program_path() {
            let exe = r"C:\Users\O'Brien\AppData\Local\Flow\Flow.exe";
            let setup = setup_script(exe);
            assert!(setup.contains(&format!("-Protocol UDP -LocalPort {DISCOVERY_PORT}")));
            assert!(setup.contains(&format!("-Protocol TCP -LocalPort {PEER_PORT}")));
            assert!(setup.contains("-Profile Any"));
            assert!(setup.contains("Remove-NetFirewallRule"));
            assert!(setup.contains(r"'C:\Users\O''Brien\AppData\Local\Flow\Flow.exe'"));
            let probe = probe_script(exe);
            assert!(
                probe.contains("exit 2") && probe.contains("exit 1") && probe.contains("exit 0")
            );
            assert!(
                !probe.contains("New-NetFirewallRule"),
                "the probe only reads"
            );
        }

        #[test]
        fn the_elevated_wrapper_carries_the_script_without_quotes_and_maps_cancel() {
            let wrapper = elevate_script(&setup_script(r"C:\Flow.exe"));
            assert!(wrapper.contains("-Verb RunAs"));
            assert!(wrapper.contains(&format!("exit {CANCELLED}")));
            let encoded = wrapper.split('\'').nth(7).expect("encoded argument");
            assert!(encoded
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b)));
        }

        #[test]
        #[ignore = "prints the scripts for reading"]
        fn print_scripts() {
            println!(
                "{}\n---\n{}",
                probe_script(r"C:\\Flow.exe"),
                setup_script(r"C:\\Flow.exe")
            );
        }

        #[test]
        fn probe_exit_codes_map_to_states() {
            assert_eq!(state_from_exit(Some(0)), Some(State::Open));
            assert_eq!(state_from_exit(Some(1)), Some(State::Missing));
            assert_eq!(state_from_exit(Some(2)), Some(State::Blocked));
            assert_eq!(state_from_exit(Some(5)), None);
            assert_eq!(state_from_exit(None), None);
        }
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use flow_core::channel::{DISCOVERY_PORT, PEER_PORT};

    use super::windows_firewall::{self as fw, State};
    use super::*;

    /// No console window flashes up for the hidden PowerShell runs.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    /// The probe starts PowerShell (about a second), and `check` is polled
    /// every 2 s: probe in the background, at most this often.
    const PROBE_EVERY: Duration = Duration::from_secs(20);

    struct Probe {
        state: Option<State>,
        at: Option<Instant>,
        running: bool,
    }

    static PROBE: Mutex<Probe> = Mutex::new(Probe {
        state: None,
        at: None,
        running: false,
    });

    fn exe() -> String {
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    }

    fn powershell(script: &str) -> std::io::Result<std::process::Output> {
        Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
            .arg(fw::encode(script))
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    }

    fn probe_now() -> Option<State> {
        match powershell(&fw::probe_script(&exe())) {
            Ok(out) => {
                let state = fw::state_from_exit(out.status.code());
                if state.is_none() {
                    tracing::warn!(
                        "could not read the Windows Firewall rules (exit {:?}): {}",
                        out.status.code(),
                        String::from_utf8_lossy(&out.stderr).trim()
                    );
                }
                state
            }
            Err(err) => {
                tracing::warn!("could not run PowerShell to read the firewall rules: {err}");
                None
            }
        }
    }

    fn store(state: Option<State>) {
        let mut probe = PROBE.lock().unwrap_or_else(|e| e.into_inner());
        probe.state = state;
        probe.at = Some(Instant::now());
        probe.running = false;
    }

    /// The last probe result; starts a background probe when it is stale.
    /// `None` until the first probe finishes, or when it cannot run.
    fn firewall_state() -> Option<State> {
        let mut probe = PROBE.lock().unwrap_or_else(|e| e.into_inner());
        let stale = probe.at.is_none_or(|at| at.elapsed() >= PROBE_EVERY);
        if stale && !probe.running {
            probe.running = true;
            std::thread::spawn(|| store(probe_now()));
        }
        probe.state
    }

    fn firewall_fix(state: State) -> HowToFix {
        let summary = match state {
            State::Blocked => "Windows Firewall is blocking Flow (its \u{201c}Allow access\u{201d} prompt was probably dismissed), so other computers can\u{2019}t reach this one.",
            _ => "Windows Firewall stops other computers from finding and connecting to this one.",
        };
        HowToFix {
            summary: summary.into(),
            steps: vec![
                FixStep::text("Click \u{201c}Set Up Automatically\u{201d} and choose Yes in the Windows prompt."),
                FixStep::text(&format!(
                    "Or allow it yourself: Windows Security \u{203a} Firewall & network protection \u{203a} Allow an app through firewall \u{203a} Flow, Private and Public. Flow uses UDP port {DISCOVERY_PORT} and TCP port {PEER_PORT}."
                )),
            ],
        }
    }

    pub fn check() -> Vec<PermissionItem> {
        match firewall_state() {
            Some(State::Open) => vec![item(INCOMING_CONNECTIONS, true, || {
                firewall_fix(State::Open)
            })],
            Some(state) => vec![item(INCOMING_CONNECTIONS, false, || firewall_fix(state))],
            // Not known yet (or unreadable): claim nothing.
            None => Vec::new(),
        }
    }

    pub fn request(_name: Option<&str>) -> Result<(), SetupError> {
        tracing::info!(target: "flow", "[SETUP] asking Windows for administrator access to allow Flow through the firewall");
        let out = powershell(&fw::elevate_script(&fw::setup_script(&exe())))
            .map_err(|err| SetupError::Unavailable(format!("PowerShell could not start: {err}")))?;
        let result = match out.status.code() {
            Some(0) => Ok(()),
            Some(fw::CANCELLED) => Err(SetupError::Cancelled),
            code => Err(SetupError::Failed(format!(
                "adding the firewall rules failed (exit {code:?})"
            ))),
        };
        // Re-probe now so the result shows at once.
        store(probe_now());
        result
    }

    pub fn one_click_setup() -> bool {
        true
    }

    pub fn settings_url(name: &str) -> Option<&'static str> {
        (name == INCOMING_CONNECTIONS).then_some("windowsdefender://network")
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::string::{CFString, CFStringRef};

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: CFStringRef;
        fn AXIsProcessTrusted() -> u8;
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> u8;
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOHIDCheckAccess(request_type: u32) -> u32;
        fn IOHIDRequestAccess(request_type: u32) -> u8;
    }

    /// `kIOHIDRequestTypeListenEvent`.
    const LISTEN_EVENT: u32 = 1;
    /// `kIOHIDAccessTypeGranted`.
    const ACCESS_GRANTED: u32 = 0;

    fn accessibility_granted() -> bool {
        // SAFETY: plain C call with no arguments.
        unsafe { AXIsProcessTrusted() != 0 }
    }

    fn input_monitoring_granted() -> bool {
        // SAFETY: plain C call with a constant argument.
        unsafe { IOHIDCheckAccess(LISTEN_EVENT) == ACCESS_GRANTED }
    }

    fn mac_fix(summary: &str, pane: &str, reopen: bool) -> HowToFix {
        let mut steps = vec![
            FixStep::text(&format!(
                "Open System Settings \u{203a} Privacy & Security \u{203a} {pane}."
            )),
            FixStep::text("Turn on Flow."),
        ];
        if reopen {
            steps.push(FixStep::text(
                "macOS asks to quit and reopen Flow. Choose \u{201c}Later\u{201d}: Flow offers a restart button.",
            ));
        }
        HowToFix {
            summary: summary.into(),
            steps,
        }
    }

    pub fn check() -> Vec<PermissionItem> {
        vec![
            item(ACCESSIBILITY, accessibility_granted(), || {
                mac_fix(
                    "Flow needs Accessibility access to type and click for you.",
                    "Accessibility",
                    false,
                )
            }),
            item(INPUT_MONITORING, input_monitoring_granted(), || {
                mac_fix(
                    "Flow needs Input Monitoring to notice your switch shortcut.",
                    "Input Monitoring",
                    true,
                )
            }),
        ]
    }

    pub fn one_click_setup() -> bool {
        false
    }

    pub fn request(name: Option<&str>) -> Result<(), SetupError> {
        if name.is_none_or(|n| n == ACCESSIBILITY) && !accessibility_granted() {
            // SAFETY: the key is a constant exported by ApplicationServices;
            // the dictionary outlives the call.
            unsafe {
                let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
                let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
                AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef());
            }
        }
        if name.is_none_or(|n| n == INPUT_MONITORING) && !input_monitoring_granted() {
            // SAFETY: plain C call with a constant argument.
            unsafe {
                IOHIDRequestAccess(LISTEN_EVENT);
            }
        }
        Ok(())
    }

    pub fn settings_url(name: &str) -> Option<&'static str> {
        match name {
            ACCESSIBILITY => Some(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            ),
            INPUT_MONITORING => {
                Some("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
            }
            LOCAL_NETWORK => {
                Some("x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork")
            }
            _ => None,
        }
    }
}
