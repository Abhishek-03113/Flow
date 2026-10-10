//! `flow-daemon`: the headless engine. Reads its configuration from
//! `FLOW_*` environment variables, starts the engine and runs until
//! Ctrl-C. The UI (or `docs/contracts/daemon-ipc.md` clients) connect over
//! the loopback WebSocket.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use flow_core::ipc::IPC_PORT;
use flow_daemon::devmode::DevMode;
use flow_daemon::discovery::tcp::DISCOVERY_PORT;
use flow_daemon::engine::{DiscoveryConfig, Engine, EngineConfig, IpcConfig};
use flow_daemon::ipc::auth;
use flow_daemon::platform_backend::NativeBackend;

#[tokio::main]
async fn main() {
    let dev = DevMode::from_env();
    // `debug_logging`'s persisted value isn't known until the engine has
    // loaded settings; the engine syncs it onto this handle.
    let logging = flow_daemon::logging::init(false, dev.trace);
    for warning in &dev.warnings {
        tracing::warn!("{warning}");
    }
    if let Some(banner) = dev.insecure_banner() {
        tracing::warn!("\n{banner}");
    }

    let mut config = EngineConfig::new(data_dir(), Arc::new(NativeBackend));
    config.device_name = std::env::var("FLOW_DEVICE_NAME").ok();
    config.security = dev.security;
    config.test_hooks = dev.test_hooks;
    config.seed_mock_parity = std::env::var_os("FLOW_DAEMON_SEED_MOCK_PARITY").is_some();
    config.logging = Some(logging);
    config.peer_listen_port = env_port("FLOW_PEER_PORT").unwrap_or(0);
    config.ipc = Some(IpcConfig {
        port: ipc_port(),
        token_path: auth::token_path(),
    });
    config.discovery = Some(DiscoveryConfig {
        port: env_port("FLOW_DISCOVERY_PORT").unwrap_or(DISCOVERY_PORT),
        targets: discovery_targets(),
    });

    let engine = match Engine::start(config).await {
        Ok(engine) => engine,
        Err(err) => fatal(&format!(
            "{err}\n(another flow-daemon may already be running on this port — \
             set FLOW_IPC_PORT to run a second instance)"
        )),
    };
    if let Some(addr) = engine.ipc_addr() {
        tracing::info!("flow-daemon listening on {addr}");
    }

    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
    engine.shutdown();
}

/// `FLOW_DATA_DIR` overrides the platform data directory — the supported
/// way to run a second instance on one machine.
fn data_dir() -> PathBuf {
    match std::env::var_os("FLOW_DATA_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => directories::ProjectDirs::from("dev", "Flow", "flow-daemon")
            .map(|dirs| dirs.data_dir().to_path_buf())
            .unwrap_or_else(|| fatal("could not determine the platform data directory")),
    }
}

/// The control-server port: `FLOW_IPC_PORT`, else [`IPC_PORT`]. An
/// unparseable value is a hard error — falling back would land a second
/// instance on the first one's port.
fn ipc_port() -> u16 {
    env_port("FLOW_IPC_PORT").unwrap_or(IPC_PORT)
}

fn env_port(name: &str) -> Option<u16> {
    match std::env::var(name) {
        Ok(raw) => Some(
            raw.parse()
                .unwrap_or_else(|_| fatal(&format!("{name} is not a valid port number: {raw:?}"))),
        ),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => fatal(&format!("{name} is not valid UTF-8")),
    }
}

/// `FLOW_DISCOVERY_TARGETS`: comma-separated `host:port` announce
/// destinations replacing LAN broadcast, to isolate instances on one host.
fn discovery_targets() -> Option<Vec<SocketAddr>> {
    let raw = std::env::var("FLOW_DISCOVERY_TARGETS").ok()?;
    Some(
        raw.split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(|part| {
                part.parse().unwrap_or_else(|_| {
                    fatal(&format!(
                        "FLOW_DISCOVERY_TARGETS has an invalid address: {part:?}"
                    ))
                })
            })
            .collect(),
    )
}

/// Prints a one-line reason to stderr and exits non-zero, for
/// unrecoverable startup misconfiguration.
fn fatal(message: &str) -> ! {
    eprintln!("flow-daemon: {message}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins down `ipc_port()`'s parsing: unset falls back to the
    /// well-known port, a valid value overrides it.
    #[test]
    fn ipc_port_defaults_and_overrides() {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os("FLOW_IPC_PORT");

        std::env::remove_var("FLOW_IPC_PORT");
        assert_eq!(ipc_port(), IPC_PORT);

        std::env::set_var("FLOW_IPC_PORT", "47999");
        assert_eq!(ipc_port(), 47999);

        match previous {
            Some(value) => std::env::set_var("FLOW_IPC_PORT", value),
            None => std::env::remove_var("FLOW_IPC_PORT"),
        }
    }
}
