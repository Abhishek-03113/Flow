//! Flow daemon entry point.
//!
//! Per vision.md §8, the daemon must work with no UI attached: it opens
//! its SQLite-backed state, starts the connection-history logger, and
//! serves the local IPC contract (`docs/contracts/daemon-ipc.md`) over a
//! WebSocket on `127.0.0.1:IPC_PORT` until the process is asked to stop.

use std::path::PathBuf;
use std::sync::Arc;

use flow_core::ipc::IPC_PORT;
use flow_daemon::hotkey;
use flow_daemon::ipc::auth;
use flow_daemon::ipc::server::handle_connection;
use flow_daemon::service::DaemonService;
use flow_daemon::storage::{history_logger, Storage};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    // `debug_logging`'s persisted value isn't known until settings load
    // below; starts at the non-debug level and gets synced once it is.
    let logging = flow_daemon::logging::init(false);

    let storage = Storage::open(db_path())
        .await
        .expect("failed to open flow-daemon database");
    let service = Arc::new(DaemonService::new(storage.clone()).await);
    let _history_logger = history_logger::spawn(&service, storage.clone());
    let _hotkey_runner = hotkey::runner::spawn(&service);
    let _debug_logging_toggle = flow_daemon::logging::spawn_debug_logging_toggle(&service, logging);

    // Every IPC connection must present this token (`auth::token_path()`)
    // as its WebSocket subprotocol — `127.0.0.1` is reachable by any
    // local process, not just the intended Flutter UI, and this is what
    // actually tells the two apart now instead of trusting the loopback
    // address alone.
    let ipc_token: Arc<str> = Arc::from(auth::load_or_generate_token());
    tracing::info!("IPC auth token: {}", auth::token_path().display());

    let listener = TcpListener::bind(("127.0.0.1", IPC_PORT))
        .await
        .unwrap_or_else(|e| panic!("failed to bind 127.0.0.1:{IPC_PORT}: {e}"));
    tracing::info!("flow-daemon listening on 127.0.0.1:{IPC_PORT}");

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, peer)) => {
                        tracing::debug!("accepted connection from {peer}");
                        let service = Arc::clone(&service);
                        let ipc_token = Arc::clone(&ipc_token);
                        tokio::spawn(async move {
                            handle_connection(stream, service, ipc_token).await;
                        });
                    }
                    Err(e) => {
                        tracing::warn!("failed to accept connection: {e}");
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("shutting down");
                break;
            }
        }
    }
}

/// The database file lives under the platform data directory (via the
/// `directories` crate) rather than the working directory, so
/// `flow-daemon` behaves the same regardless of where it's launched from.
fn db_path() -> PathBuf {
    let dirs = directories::ProjectDirs::from("dev", "Flow", "flow-daemon")
        .expect("could not determine the platform data directory");
    let dir = dirs.data_dir();
    std::fs::create_dir_all(dir).expect("failed to create the data directory");
    dir.join("flow.db")
}
