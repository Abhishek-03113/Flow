//! The command/event bridge between the webview and the embedded engine.
//!
//! Commands go through the engine's single dispatcher
//! (`flow_daemon::ipc::dispatch::run_command`), the same function the
//! WebSocket server uses. Events come from the engine's single state stream
//! (`flow_daemon::ipc::events::StateStream`) and fan out to every
//! [`StateObserver`] (webview, tray, notifications, OS integrations).

use std::sync::{Arc, Mutex};

use flow_core::ipc::ErrorPayload;
use flow_daemon::engine::EngineHandle;
use flow_daemon::ipc::dispatch::run_command;
use flow_daemon::ipc::events::{StateEvent, StateStream, RESYNC_COMMAND};
use flow_daemon::service::DaemonService;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::shell;

/// Name of the Tauri event carrying `{ event, payload }` engine pushes.
pub const EVENT_CHANNEL: &str = "flow://event";

/// Something that reacts to engine state changes.
pub trait StateObserver: Send + Sync {
    fn on_event(&self, app: &AppHandle, event: &StateEvent);
}

/// Forwards every state event to the webview.
pub struct WebviewForwarder;

impl StateObserver for WebviewForwarder {
    fn on_event(&self, app: &AppHandle, event: &StateEvent) {
        if let Err(err) = app.emit(
            EVENT_CHANNEL,
            json!({ "event": event.name, "payload": event.payload }),
        ) {
            tracing::debug!("could not emit {}: {err}", event.name);
        }
    }
}

/// The running engine, shared as Tauri state.
pub struct Engine {
    service: Arc<DaemonService>,
    handle: Mutex<Option<EngineHandle>>,
    observers: Vec<Box<dyn StateObserver>>,
}

impl Engine {
    pub fn new(handle: EngineHandle, observers: Vec<Box<dyn StateObserver>>) -> Self {
        Self {
            service: Arc::clone(handle.service()),
            handle: Mutex::new(Some(handle)),
            observers,
        }
    }

    pub fn service(&self) -> &Arc<DaemonService> {
        &self.service
    }

    /// Stops the engine (idempotent): called when the app quits.
    pub fn shutdown(&self) {
        let handle = self.handle.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(handle) = handle {
            tracing::info!("stopping the engine");
            handle.shutdown();
        }
    }

    fn publish(&self, app: &AppHandle, events: &[StateEvent]) {
        for event in events {
            for observer in &self.observers {
                observer.on_event(app, event);
            }
        }
    }

    /// Every current state event, to every observer (the `resync` command,
    /// and the first push at startup).
    pub fn resync(&self, app: &AppHandle) {
        let mut stream = StateStream::new(&self.service);
        self.publish(app, &stream.snapshot(&self.service));
    }

    /// Streams engine changes to the observers for as long as the engine lives.
    pub fn start_pump(self: &Arc<Self>, app: AppHandle) {
        let engine = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let mut stream = StateStream::new(&engine.service);
            engine.publish(&app, &stream.snapshot(&engine.service));
            while let Some(event) = stream.next().await {
                engine.publish(&app, std::slice::from_ref(&event));
            }
        });
    }
}

/// Runs one command through the engine's dispatcher. A plain ack is
/// `null`; commands with an answer return it (the wire ack's `result`).
pub async fn execute(
    service: &DaemonService,
    command: &str,
    payload: Value,
) -> Result<Value, ErrorPayload> {
    run_command(service, command, payload)
        .await
        .map(|result| result.unwrap_or(Value::Null))
}

/// The one command the UI invokes: same names, payloads, errors and
/// results as the daemon IPC contract (`docs/contracts/daemon-ipc.md`).
#[tauri::command]
pub async fn flow_command(
    app: AppHandle,
    engine: State<'_, Arc<Engine>>,
    command: String,
    payload: Value,
) -> Result<Value, ErrorPayload> {
    tracing::debug!(%command, "flow_command");
    if command == RESYNC_COMMAND {
        engine.resync(&app);
        return Ok(Value::Null);
    }
    let result = execute(engine.service(), &command, payload).await?;
    shell::after_command(&app, &command, &result);
    Ok(result)
}

/// Looks up the engine from any handle.
pub fn engine_of(app: &AppHandle) -> Option<Arc<Engine>> {
    app.try_state::<Arc<Engine>>().map(|s| Arc::clone(&s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow_daemon::storage::Storage;

    async fn service() -> DaemonService {
        let storage = Storage::open_in_memory().await.expect("open db");
        DaemonService::new_seeded_for_test(storage).await
    }

    #[tokio::test]
    async fn a_plain_command_acks_with_null() {
        let service = service().await;
        let result = execute(&service, "start_pairing", Value::Null).await;
        assert_eq!(result.unwrap(), Value::Null);
    }

    #[tokio::test]
    async fn a_command_with_an_answer_returns_it() {
        let service = service().await;
        let result = execute(&service, "get_keymap", json!({ "os": "windows" }))
            .await
            .unwrap();
        assert_eq!(result["os"], "windows");
    }

    #[tokio::test]
    async fn errors_carry_the_contract_code() {
        let service = service().await;
        let err = execute(&service, "remove_device", json!({ "device_id": "d1" }))
            .await
            .unwrap_err();
        assert_eq!(err.code, "device_not_removable");
        let err = execute(&service, "nope", Value::Null).await.unwrap_err();
        assert_eq!(err.code, "unknown_command");
    }
}
