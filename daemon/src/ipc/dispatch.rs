//! `IpcRequest -> DaemonService` call, matching `req.command` against the
//! 9 known commands from `sharedContractConstants.commands`
//! (`daemon/todos.json` task C2). The single place a raw request string
//! is matched against a command name and a JSON payload is deserialized
//! into a concrete argument type.

use flow_core::ipc::{ErrorPayload, IpcRequest, IpcResponse};
use flow_core::settings::SettingsPatch;
use flow_core::switch_key::SwitchKeyBinding;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::service::DaemonService;

/// Dispatches one request, mapping the `DaemonService` call's
/// `Ok(())`/`Err(FlowError)` into `IpcResponse::Ack`/`IpcResponse::Err`
/// with the original request's `id` echoed back exactly once, per
/// `daemon-ipc.md`'s "`id` on a command... echoed back in exactly one
/// ack" rule.
pub async fn dispatch(service: &DaemonService, req: IpcRequest) -> IpcResponse {
    match handle(service, &req.command, req.payload).await {
        Ok(()) => IpcResponse::Ack {
            id: req.id,
            ok: true,
        },
        Err(error) => IpcResponse::Err {
            id: req.id,
            ok: false,
            error,
        },
    }
}

async fn handle(
    service: &DaemonService,
    command: &str,
    payload: Value,
) -> Result<(), ErrorPayload> {
    match command {
        "switch_active_device" => {
            let args: DeviceIdPayload = parse_payload(payload)?;
            service
                .switch_active_device(&args.device_id)
                .await
                .map_err(ErrorPayload::from)
        }
        "remove_device" => {
            let args: DeviceIdPayload = parse_payload(payload)?;
            service
                .remove_device(&args.device_id)
                .await
                .map_err(ErrorPayload::from)
        }
        "start_pairing" => service.start_pairing().await.map_err(ErrorPayload::from),
        "cancel_pairing" => service.cancel_pairing().await.map_err(ErrorPayload::from),
        "pair_with_candidate" => {
            let args: CandidateIdPayload = parse_payload(payload)?;
            service
                .pair_with_candidate(&args.candidate_id)
                .await
                .map_err(ErrorPayload::from)
        }
        "set_switch_key" => {
            let binding: SwitchKeyBinding = parse_payload(payload)?;
            service
                .set_switch_key(binding)
                .await
                .map_err(ErrorPayload::from)
        }
        "update_settings" => {
            let patch: SettingsPatch = parse_payload(payload)?;
            service
                .update_settings(patch)
                .await
                .map_err(ErrorPayload::from)
        }
        "reset_settings" => service.reset_settings().await.map_err(ErrorPayload::from),
        "request_permission" => service
            .request_permission()
            .await
            .map_err(ErrorPayload::from),
        other => Err(unknown_command(other)),
    }
}

#[derive(serde::Deserialize)]
struct DeviceIdPayload {
    device_id: String,
}

#[derive(serde::Deserialize)]
struct CandidateIdPayload {
    candidate_id: String,
}

/// Deserializes `payload` into `T`, mapping a shape mismatch to a stable
/// `invalid_payload` error response instead of panicking — the reverse
/// of `daemon-ipc.md`'s forward-compat note about unrecognized codes: a
/// client sending a payload for a command it doesn't fully match must
/// fail cleanly too.
fn parse_payload<T: DeserializeOwned>(payload: Value) -> Result<T, ErrorPayload> {
    serde_json::from_value(payload).map_err(|e| ErrorPayload {
        code: "invalid_payload".to_string(),
        message: format!("invalid payload: {e}"),
    })
}

fn unknown_command(command: &str) -> ErrorPayload {
    ErrorPayload {
        code: "unknown_command".to_string(),
        message: format!("unrecognized command: {command}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use flow_core::error::FlowError;
    use serde_json::json;

    async fn service() -> DaemonService {
        let storage = Storage::open_in_memory().await.expect("open db");
        DaemonService::new(storage).await
    }

    #[tokio::test(start_paused = true)]
    async fn switch_active_device_round_trips_through_dispatch() {
        let service = service().await;
        let req = IpcRequest {
            id: "req-1".to_string(),
            command: "switch_active_device".to_string(),
            payload: json!({ "device_id": "d2" }),
        };
        let response = dispatch(&service, req).await;
        assert_eq!(
            response,
            IpcResponse::Ack {
                id: "req-1".to_string(),
                ok: true
            }
        );
    }

    #[tokio::test]
    async fn a_rejected_command_returns_the_matching_error_code() {
        let service = service().await;
        let req = IpcRequest {
            id: "req-2".to_string(),
            command: "remove_device".to_string(),
            payload: json!({ "device_id": "d1" }),
        };
        let response = dispatch(&service, req).await;
        assert_eq!(
            response,
            IpcResponse::Err {
                id: "req-2".to_string(),
                ok: false,
                error: ErrorPayload {
                    code: "device_not_removable".to_string(),
                    message: FlowError::DeviceNotRemovable(flow_core::device::DeviceId(
                        "d1".to_string()
                    ))
                    .to_string(),
                }
            }
        );
    }

    #[tokio::test]
    async fn an_unrecognized_command_fails_cleanly_not_a_panic() {
        let service = service().await;
        let req = IpcRequest {
            id: "req-3".to_string(),
            command: "not_a_real_command".to_string(),
            payload: Value::Null,
        };
        let response = dispatch(&service, req).await;
        match response {
            IpcResponse::Err { ok, error, .. } => {
                assert!(!ok);
                assert_eq!(error.code, "unknown_command");
            }
            other => panic!("expected Err, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_malformed_payload_fails_cleanly_not_a_deserialize_panic() {
        let service = service().await;
        let req = IpcRequest {
            id: "req-4".to_string(),
            command: "switch_active_device".to_string(),
            // missing the required device_id field
            payload: json!({}),
        };
        let response = dispatch(&service, req).await;
        match response {
            IpcResponse::Err { ok, error, .. } => {
                assert!(!ok);
                assert_eq!(error.code, "invalid_payload");
            }
            other => panic!("expected Err, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn commands_with_no_payload_ignore_a_null_payload() {
        let service = service().await;
        let req = IpcRequest {
            id: "req-5".to_string(),
            command: "start_pairing".to_string(),
            payload: Value::Null,
        };
        let response = dispatch(&service, req).await;
        assert_eq!(
            response,
            IpcResponse::Ack {
                id: "req-5".to_string(),
                ok: true
            }
        );
    }
}
