//! `up` mode: engines on virtual platforms with real control servers, for
//! driving the real UI against them (Playwright E2E). Prints one JSON line
//! per device and runs until interrupted.

use std::io::Write;
use std::path::PathBuf;

use flow_core::control::Role;
use flow_core::device::{HostOs, ScreenSize};
use flow_daemon::devmode::SecurityMode;
use serde::Serialize;

use crate::scenario::parse_os_short;
use crate::world::{DeviceSpec, IpcMode, LinkExpect, LinkSpec, World, WorldOptions};

/// What a client needs to connect to one device's control server.
#[derive(Debug, Serialize)]
pub struct ManifestEntry {
    pub name: String,
    pub os: HostOs,
    pub role: Role,
    pub ws_url: String,
    pub token: String,
}

#[derive(Debug)]
pub struct UpOptions {
    pub devices: Vec<DeviceSpec>,
    pub paired: bool,
    pub base_port: Option<u16>,
    pub manifest: Option<PathBuf>,
    pub security: SecurityMode,
}

/// `Name:os:role,Name:os:role` (role `controller`|`controlled`).
pub fn parse_devices(text: &str) -> Result<Vec<DeviceSpec>, String> {
    text.split(',')
        .map(|part| {
            let fields: Vec<&str> = part.trim().split(':').collect();
            let [name, os, role] = fields[..] else {
                return Err(format!("{part:?}: expected Name:os:role"));
            };
            let os = parse_os_short(os).ok_or_else(|| format!("{part:?}: unknown OS {os:?}"))?;
            let role = match role {
                "controller" => Role::Controller,
                "controlled" => Role::Controlled,
                other => return Err(format!("{part:?}: unknown role {other:?}")),
            };
            Ok(DeviceSpec {
                name: name.to_string(),
                os,
                role,
                screen: ScreenSize {
                    width: 1920,
                    height: 1080,
                },
            })
        })
        .collect()
}

/// Starts the world and returns it with the manifest of its devices.
pub async fn start(options: &UpOptions) -> Result<(World, Vec<ManifestEntry>), String> {
    let opts = WorldOptions {
        security: options.security,
        ipc: options.base_port.map_or(IpcMode::Ephemeral, IpcMode::Base),
        test_hooks: true,
        auto_consent: options.paired,
        ..WorldOptions::default()
    };
    let mut world = World::start_unlinked(options.devices.clone(), opts).await?;
    world.connect_all_discovery()?;
    if options.paired {
        let controllers: Vec<_> = options
            .devices
            .iter()
            .filter(|d| d.role == Role::Controller)
            .collect();
        for controller in controllers {
            for ctd in options
                .devices
                .iter()
                .filter(|d| d.role == Role::Controlled)
            {
                let link = LinkSpec {
                    a: controller.name.clone(),
                    b: ctd.name.clone(),
                    expect: LinkExpect::Live,
                };
                world.link(&link).await?;
            }
        }
        world.release_consent();
    }
    let manifest = world
        .devices()
        .iter()
        .map(|device| {
            let addr = device.ipc_addr().ok_or("no control server")?;
            Ok(ManifestEntry {
                name: device.spec.name.clone(),
                os: device.spec.os,
                role: device.spec.role,
                ws_url: format!("ws://{addr}"),
                token: device.token().ok_or("no token")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok((world, manifest))
}

/// Prints the manifest (a JSON line per device) and writes it to the
/// manifest file as a JSON array when asked.
pub fn publish(manifest: &[ManifestEntry], file: Option<&PathBuf>) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    for entry in manifest {
        let line = serde_json::to_string(entry).map_err(|e| e.to_string())?;
        writeln!(stdout, "{line}").map_err(|e| e.to_string())?;
    }
    stdout.flush().map_err(|e| e.to_string())?;
    if let Some(path) = file {
        let json = serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}
