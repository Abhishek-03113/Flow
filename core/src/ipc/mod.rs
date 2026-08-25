//! Transport-agnostic wire envelope for local daemon<->UI IPC
//! (`docs/contracts/daemon-ipc.md` "Wire envelope"). Lives in `flow-core`,
//! not `flow-daemon`, per this crate's own doc comment: shared vocabulary
//! any future transport implementation builds on, not just
//! `flow-daemon`'s own WebSocket server (track C3).

use serde::{Deserialize, Serialize};

use crate::error::FlowError;

/// Loopback-TCP port the daemon's WebSocket IPC server listens on.
pub const IPC_PORT: u16 = 47823;

/// A command sent from the UI to the daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcRequest {
    pub id: String,
    pub command: String,
    pub payload: serde_json::Value,
}

/// A machine-readable/human-readable error pair, matching
/// `DaemonCommandException(code, message)` on the Dart side
/// (`flutter/lib/domain/daemon_command_exception.dart`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
}

impl From<FlowError> for ErrorPayload {
    fn from(error: FlowError) -> Self {
        Self {
            code: error.code().to_string(),
            message: error.to_string(),
        }
    }
}

/// Everything the daemon can send back over the IPC connection: an
/// ack/error for a specific command (matched to the request by `id`), or
/// an unsolicited state-change event (no `id`).
///
/// `#[serde(untagged)]`, with `Err` listed before `Ack`: both share
/// `id`/`ok`, and only `Err` additionally requires `error`. Untagged
/// deserialization tries variants in declaration order, and struct
/// deserialization otherwise ignores JSON fields it doesn't recognize —
/// so if `Ack` were tried first, an `Err` payload's extra `error` field
/// would be silently dropped and it would wrongly deserialize as `Ack`.
/// Trying `Err` first means it only matches when `error` is actually
/// present; anything else falls through to `Ack`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IpcResponse {
    Err {
        id: String,
        ok: bool,
        error: ErrorPayload,
    },
    Ack {
        id: String,
        ok: bool,
    },
    Event {
        event: String,
        payload: serde_json::Value,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_matches_the_daemon_ipc_md_example() {
        let request = IpcRequest {
            id: "req-17".to_string(),
            command: "switch_active_device".to_string(),
            payload: json!({ "device_id": "d2" }),
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({ "id": "req-17", "command": "switch_active_device", "payload": { "device_id": "d2" } })
        );
    }

    #[test]
    fn ack_matches_the_daemon_ipc_md_example() {
        let ack = IpcResponse::Ack {
            id: "req-17".to_string(),
            ok: true,
        };
        assert_eq!(
            serde_json::to_value(&ack).unwrap(),
            json!({ "id": "req-17", "ok": true })
        );
    }

    #[test]
    fn err_matches_the_daemon_ipc_md_example() {
        let err = IpcResponse::Err {
            id: "req-19".to_string(),
            ok: false,
            error: ErrorPayload {
                code: "device_not_found".to_string(),
                message: "no device d2".to_string(),
            },
        };
        assert_eq!(
            serde_json::to_value(&err).unwrap(),
            json!({
                "id": "req-19",
                "ok": false,
                "error": { "code": "device_not_found", "message": "no device d2" }
            })
        );
    }

    #[test]
    fn event_matches_the_daemon_ipc_md_example() {
        let event = IpcResponse::Event {
            event: "devices_changed".to_string(),
            payload: json!([]),
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({ "event": "devices_changed", "payload": [] })
        );
    }

    /// Proves the untagged-variant ordering actually round-trips
    /// correctly: an `Err` response must deserialize back as `Err`, not
    /// silently as `Ack` with its `error` field dropped.
    #[test]
    fn err_round_trips_and_is_not_mistaken_for_ack() {
        let original = IpcResponse::Err {
            id: "req-19".to_string(),
            ok: false,
            error: ErrorPayload {
                code: "device_not_found".to_string(),
                message: "no device d2".to_string(),
            },
        };
        let json = serde_json::to_value(&original).unwrap();
        let round_tripped: IpcResponse = serde_json::from_value(json).unwrap();
        assert_eq!(round_tripped, original);
        assert!(matches!(round_tripped, IpcResponse::Err { .. }));
    }

    #[test]
    fn ack_round_trips_and_is_not_mistaken_for_err() {
        let original = IpcResponse::Ack {
            id: "req-17".to_string(),
            ok: true,
        };
        let json = serde_json::to_value(&original).unwrap();
        let round_tripped: IpcResponse = serde_json::from_value(json).unwrap();
        assert_eq!(round_tripped, original);
        assert!(matches!(round_tripped, IpcResponse::Ack { .. }));
    }

    #[test]
    fn error_payload_from_flow_error_carries_the_code_and_display_message() {
        let error = FlowError::DeviceNotFound(crate::device::DeviceId("d2".to_string()));
        let payload: ErrorPayload = error.clone().into();
        assert_eq!(payload.code, "device_not_found");
        assert_eq!(payload.message, error.to_string());
    }
}
