//! What the OS must allow before Flow can share input, probed for real.
//!
//! * macOS: Accessibility (post events) and Input Monitoring (listen to
//!   events) are separate grants; each can be requested to show the OS prompt.
//! * Windows: nothing to grant at runtime.
//! * Linux: read access to `/dev/input/event*` and write access to
//!   `/dev/uinput`, normally via the `input` group and a udev rule.
//!
//! The macOS and Windows branches are compile-checked (cross clippy) but
//! have not run on real hardware; they are kept deliberately small.

use flow_core::permission::PermissionItem;
#[cfg(not(target_os = "windows"))]
use flow_core::permission::{FixStep, HowToFix};

pub const ACCESSIBILITY: &str = "Accessibility";
pub const INPUT_MONITORING: &str = "Input Monitoring";
pub const READ_INPUT: &str = "Read keyboard and mouse";
pub const CREATE_INPUT: &str = "Create virtual input device";

#[cfg(not(target_os = "windows"))]
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

/// Triggers the OS permission prompt where one exists (macOS); a no-op
/// elsewhere. `name` limits it to one item; `None` asks for everything
/// still missing.
pub fn request(name: Option<&str>) {
    imp::request(name);
}

/// The OS settings pane (or `None`) where the user can grant `name`.
pub fn settings_url(name: &str) -> Option<&'static str> {
    imp::settings_url(name)
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::fs::OpenOptions;

    fn input_group_fix() -> HowToFix {
        HowToFix {
            summary: "Flow needs your account to be in the \u{201c}input\u{201d} group.".into(),
            steps: vec![
                FixStep::command("Add yourself to the group:", "sudo usermod -aG input $USER"),
                FixStep::text("Log out and back in."),
            ],
        }
    }

    fn uinput_fix() -> HowToFix {
        HowToFix {
            summary: "Flow needs permission to create a virtual keyboard and mouse.".into(),
            steps: vec![
                FixStep::command(
                    "Allow the input group to use /dev/uinput:",
                    "echo 'KERNEL==\"uinput\", GROUP=\"input\", MODE=\"0660\"' | sudo tee /etc/udev/rules.d/99-flow.rules && sudo udevadm control --reload && sudo udevadm trigger",
                ),
                FixStep::text("Make sure you are in the \u{201c}input\u{201d} group, then log out and back in."),
            ],
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

    pub fn check() -> Vec<PermissionItem> {
        vec![
            item(READ_INPUT, can_read_an_input_device(), input_group_fix),
            item(
                CREATE_INPUT,
                OpenOptions::new().write(true).open("/dev/uinput").is_ok(),
                uinput_fix,
            ),
        ]
    }

    pub fn request(_name: Option<&str>) {}

    pub fn settings_url(_name: &str) -> Option<&'static str> {
        None
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use super::*;

    pub fn check() -> Vec<PermissionItem> {
        Vec::new()
    }

    pub fn request(_name: Option<&str>) {}

    pub fn settings_url(_name: &str) -> Option<&'static str> {
        // Windows has no input permission; the privacy page is the closest
        // useful destination if a security product blocks Flow.
        Some("ms-settings:privacy")
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
            steps.push(FixStep::text("Reopen Flow if it does not pick it up."));
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

    pub fn request(name: Option<&str>) {
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
    }

    pub fn settings_url(name: &str) -> Option<&'static str> {
        match name {
            ACCESSIBILITY => Some(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            ),
            INPUT_MONITORING => {
                Some("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
            }
            _ => None,
        }
    }
}
