//! Flow desktop shell (Tauri 2).
//!
//! Track C scaffold: window + tray skeleton and the single command/event
//! bridge the frontend talks to. Track E embeds the engine behind
//! [`flow_command`] and forwards engine events through [`emit_event`]; until
//! then every command fails with `engine_not_embedded`.

mod tray;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

/// Name of the Tauri event carrying `{ event, payload }` engine pushes.
pub const EVENT_CHANNEL: &str = "flow://event";

/// Mirrors the daemon IPC `ErrorPayload` (`{ code, message }`).
#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
}

/// The one command the UI invokes: same names/payloads as the daemon IPC
/// contract (`docs/contracts/daemon-ipc.md`, contract 0.2.0 additions).
#[tauri::command]
fn flow_command(command: String, payload: Value) -> Result<(), CommandError> {
    let _ = payload;
    Err(CommandError {
        code: "engine_not_embedded",
        message: format!("engine not embedded yet (command: {command})"),
    })
}

/// Forwards an engine event to the frontend as `flow://event`.
pub fn emit_event(app: &AppHandle, event: &str, payload: Value) -> tauri::Result<()> {
    app.emit(EVENT_CHANNEL, json!({ "event": event, "payload": payload }))
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![flow_command])
        .setup(|app| {
            tray::install(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Flow");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_fail_until_the_engine_is_embedded() {
        let err = flow_command("switch_to".into(), json!({ "target": "local" })).unwrap_err();
        assert_eq!(err.code, "engine_not_embedded");
        assert!(err.message.contains("switch_to"));
    }
}
