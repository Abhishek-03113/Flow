//! The one place an [`EngineConfig`] is built from the process
//! environment. The headless `flow-daemon` binary and the desktop app both
//! call [`engine_config`], so they can never drift apart on data
//! directories, ports, discovery or dev switches.
//!
//! Variables (all optional): `FLOW_DATA_DIR`, `FLOW_CONFIG_DIR`,
//! `FLOW_DEVICE_NAME`, `FLOW_IPC_PORT`, `FLOW_IPC_TOKEN_PATH`,
//! `FLOW_PEER_PORT`, `FLOW_DISCOVERY_PORT`, `FLOW_DISCOVERY_TARGETS`,
//! `FLOW_DAEMON_SEED_MOCK_PARITY`, `FLOW_DEV_WS` (embedded only) and the
//! dev switches read by [`DevMode`].

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use flow_core::ipc::IPC_PORT;
use thiserror::Error;

use crate::devmode::DevMode;
use crate::discovery::tcp::DISCOVERY_PORT;
use crate::engine::{DiscoveryConfig, EngineConfig, IpcConfig};
use crate::ipc::auth;
use crate::logging::LoggingHandle;
use crate::platform_backend::PlatformBackend;

/// How the engine is hosted, which decides whether it exposes the
/// loopback control server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// `flow-daemon`: always serves the WebSocket control channel.
    Headless,
    /// The desktop app: the UI talks to the engine in-process, so there is
    /// no local socket unless `FLOW_DEV_WS=1` asks for one (debugging).
    Embedded,
}

#[derive(Debug, Error)]
#[error("{0}")]
pub struct ConfigError(String);

/// Builds the engine configuration for this process from `FLOW_*`
/// variables.
pub fn engine_config(
    profile: Profile,
    backend: Arc<dyn PlatformBackend>,
    dev: &DevMode,
    logging: Option<LoggingHandle>,
) -> Result<EngineConfig, ConfigError> {
    let mut config = EngineConfig::new(data_dir()?, backend);
    config.device_name = std::env::var("FLOW_DEVICE_NAME").ok();
    config.security = dev.security;
    config.test_hooks = dev.test_hooks;
    config.seed_mock_parity = std::env::var_os("FLOW_DAEMON_SEED_MOCK_PARITY").is_some();
    config.logging = logging;
    config.peer_listen_port = env_port("FLOW_PEER_PORT")?.unwrap_or(0);
    config.keymap_dir = flow_platform::keymap::overrides_dir();
    config.discovery = Some(DiscoveryConfig {
        port: env_port("FLOW_DISCOVERY_PORT")?.unwrap_or(DISCOVERY_PORT),
        targets: discovery_targets()?,
        announce_interval: Duration::from_secs(5),
    });
    if serve_ipc(profile) {
        config.ipc = Some(IpcConfig {
            port: ipc_port()?,
            token_path: auth::token_path(),
        });
    }
    Ok(config)
}

fn serve_ipc(profile: Profile) -> bool {
    match profile {
        Profile::Headless => true,
        Profile::Embedded => std::env::var("FLOW_DEV_WS")
            .map(|v| matches!(v.trim(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false),
    }
}

/// `FLOW_DATA_DIR` overrides the platform data directory — the supported
/// way to run a second instance on one machine.
pub fn data_dir() -> Result<PathBuf, ConfigError> {
    match std::env::var_os("FLOW_DATA_DIR") {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => directories::ProjectDirs::from("dev", "Flow", "flow-daemon")
            .map(|dirs| dirs.data_dir().to_path_buf())
            .ok_or_else(|| ConfigError("could not determine the platform data directory".into())),
    }
}

/// The control-server port: `FLOW_IPC_PORT`, else [`IPC_PORT`]. An
/// unparseable value is a hard error — falling back would land a second
/// instance on the first one's port.
pub fn ipc_port() -> Result<u16, ConfigError> {
    Ok(env_port("FLOW_IPC_PORT")?.unwrap_or(IPC_PORT))
}

fn env_port(name: &str) -> Result<Option<u16>, ConfigError> {
    match std::env::var(name) {
        Ok(raw) => raw
            .parse()
            .map(Some)
            .map_err(|_| ConfigError(format!("{name} is not a valid port number: {raw:?}"))),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(ConfigError(format!("{name} is not valid UTF-8")))
        }
    }
}

/// `FLOW_DISCOVERY_TARGETS`: comma-separated `host:port` announce
/// destinations replacing LAN broadcast, to isolate instances on one host.
fn discovery_targets() -> Result<Option<Vec<SocketAddr>>, ConfigError> {
    let Ok(raw) = std::env::var("FLOW_DISCOVERY_TARGETS") else {
        return Ok(None);
    };
    raw.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse().map_err(|_| {
                ConfigError(format!(
                    "FLOW_DISCOVERY_TARGETS has an invalid address: {part:?}"
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ipc_port()`'s parsing: unset falls back to the well-known port, a
    /// valid value overrides it, junk is an error.
    #[test]
    fn ipc_port_defaults_overrides_and_rejects_junk() {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os("FLOW_IPC_PORT");

        std::env::remove_var("FLOW_IPC_PORT");
        assert_eq!(ipc_port().unwrap(), IPC_PORT);
        std::env::set_var("FLOW_IPC_PORT", "47999");
        assert_eq!(ipc_port().unwrap(), 47999);
        std::env::set_var("FLOW_IPC_PORT", "nope");
        assert!(ipc_port().is_err());

        match previous {
            Some(value) => std::env::set_var("FLOW_IPC_PORT", value),
            None => std::env::remove_var("FLOW_IPC_PORT"),
        }
    }

    #[test]
    fn only_the_headless_profile_serves_ipc_by_default() {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("FLOW_DEV_WS");
        assert!(serve_ipc(Profile::Headless));
        assert!(!serve_ipc(Profile::Embedded));
        std::env::set_var("FLOW_DEV_WS", "1");
        assert!(serve_ipc(Profile::Embedded));
        std::env::remove_var("FLOW_DEV_WS");
    }
}
