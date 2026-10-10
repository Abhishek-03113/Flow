//! `flow-daemon`: the headless engine for servers, CI and the test
//! harness. Reads its configuration from `FLOW_*` environment variables
//! (see `flow_daemon::env_config`), starts the engine and runs until
//! Ctrl-C. A client connects over the loopback WebSocket; the desktop app
//! embeds the same engine in-process instead.

use std::sync::Arc;

use flow_daemon::devmode::DevMode;
use flow_daemon::engine::Engine;
use flow_daemon::env_config::{self, Profile};
use flow_daemon::platform_backend::NativeBackend;

#[tokio::main]
async fn main() {
    let dev = DevMode::from_env();
    // `debug_logging`'s persisted value isn't known until the engine has
    // loaded settings; the engine syncs it onto this handle.
    let logging = flow_daemon::logging::init(false, dev.trace);
    dev.announce();

    let config = env_config::engine_config(
        Profile::Headless,
        Arc::new(NativeBackend),
        &dev,
        Some(logging),
    )
    .unwrap_or_else(|err| fatal(&err.to_string()));

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

/// Prints a one-line reason to stderr and exits non-zero, for
/// unrecoverable startup misconfiguration.
fn fatal(message: &str) -> ! {
    eprintln!("flow-daemon: {message}");
    std::process::exit(1);
}
