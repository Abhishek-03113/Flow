//! The single command dispatcher: the one place a command name and its JSON
//! payload become a `DaemonService` call. Every transport (the WebSocket
//! server, the desktop app's Tauri bridge) goes through [`run_command`];
//! [`dispatch`] only wraps its result in the wire envelope.

use flow_core::ipc::{ErrorPayload, IpcRequest, IpcResponse};
use flow_core::settings::SettingsPatch;
use flow_core::switch_key::SwitchKeyBinding;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::events::RESYNC_COMMAND;
use crate::service::DaemonService;

/// Dispatches one request, mapping the `DaemonService` call's
/// `Ok(())`/`Err(FlowError)` into `IpcResponse::Ack`/`IpcResponse::Err`
/// with the original request's `id` echoed back exactly once, per
/// `daemon-ipc.md`'s "`id` on a command... echoed back in exactly one
/// ack" rule.
pub async fn dispatch(service: &DaemonService, req: IpcRequest) -> IpcResponse {
    match run_command(service, &req.command, req.payload).await {
        Ok(None) => IpcResponse::Ack {
            id: req.id,
            ok: true,
        },
        Ok(Some(result)) => IpcResponse::Data {
            id: req.id,
            ok: true,
            result,
        },
        Err(error) => IpcResponse::Err {
            id: req.id,
            ok: false,
            error,
        },
    }
}

/// Runs one command. `Ok(None)` is a plain ack; `Ok(Some(_))` carries the
/// command's answer (the ack's `result`). Transport-agnostic.
pub async fn run_command(
    service: &DaemonService,
    command: &str,
    payload: Value,
) -> Result<Option<Value>, ErrorPayload> {
    // Commands with an answer return it directly; the rest are acks only.
    match command {
        "get_keymap" => {
            let args: OsPayload = parse_payload(payload)?;
            return Ok(Some(service.keymap_for(args.os)));
        }
        // The engine decides *what* to open; a headless client just gets
        // the path/URL, the desktop app's shell opens it.
        "reveal_config_dir" => {
            let path = service.reveal_config_dir().map_err(ErrorPayload::from)?;
            return Ok(Some(json!({ "path": path.display().to_string() })));
        }
        "debug_virtual_io" => {
            // Test hooks only, like `debug_inject_input`: what the virtual
            // OS under this engine received and had injected.
            if !service.test_hooks_enabled() {
                return Err(unknown_command("debug_virtual_io"));
            }
            let args: VirtualIoPayload = if payload.is_null() {
                VirtualIoPayload::default()
            } else {
                parse_payload(payload)?
            };
            return Ok(service.debug_virtual_io(args.clear));
        }
        "export_diagnostics" => {
            let path = service
                .export_diagnostics()
                .await
                .map_err(ErrorPayload::from)?;
            return Ok(Some(json!({ "path": path.display().to_string() })));
        }
        "open_permission_settings" => {
            let args: NamePayload = parse_payload(payload)?;
            let url = service
                .permission_settings_url(&args.name)
                .await
                .map_err(ErrorPayload::from)?;
            return Ok(Some(json!({ "url": url })));
        }
        _ => {}
    }
    handle_ack_only(service, command, payload)
        .await
        .map(|()| None)
}

async fn handle_ack_only(
    service: &DaemonService,
    command: &str,
    payload: Value,
) -> Result<(), ErrorPayload> {
    match command {
        "switch_to" => {
            let args: TargetPayload = parse_payload(payload)?;
            service
                .switch_to(&args.target)
                .await
                .map_err(ErrorPayload::from)
        }
        "switch_active_device" => {
            let args: DeviceIdPayload = parse_payload(payload)?;
            service
                .switch_active_device(&args.device_id)
                .await
                .map_err(ErrorPayload::from)
        }
        "release_control" => service.release_control().await.map_err(ErrorPayload::from),
        "set_role" => {
            let args: RolePayload = parse_payload(payload)?;
            service
                .set_role(args.role)
                .await
                .map_err(ErrorPayload::from)
        }
        "set_screen_layout" => {
            let args: LayoutPayload = parse_payload(payload)?;
            service
                .set_screen_layout(args.layout)
                .await
                .map_err(ErrorPayload::from)
        }
        "set_remap_rules" => {
            let args: RemapRulesPayload = parse_payload(payload)?;
            service
                .set_remap_rules(args.rules)
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
        "respond_to_pairing_request" => {
            let args: RespondToPairingRequestPayload = parse_payload(payload)?;
            service
                .respond_to_pairing_request(&args.request_id, args.decision)
                .await
                .map_err(ErrorPayload::from)
        }
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
        "retry_connection" => service.retry_connection().await.map_err(ErrorPayload::from),
        // "Check Again": re-run the OS permission probe now.
        "recheck_permissions" => {
            service.refresh_permissions().await;
            Ok(())
        }
        "cancel_reconnect" => service.cancel_reconnect().await.map_err(ErrorPayload::from),
        // The state re-send itself belongs to the transport (it owns the
        // sink); the command is part of the contract, so ack it here.
        RESYNC_COMMAND => Ok(()),
        "debug_inject_input" => {
            // Test-hook-only (`FLOW_TEST_HOOKS`). When the hooks are off
            // this is not part of the contract at all — reported exactly
            // like any other unknown command so a production client sees
            // no difference.
            if !service.test_hooks_enabled() {
                return Err(unknown_command("debug_inject_input"));
            }
            let event: flow_core::protocol::InputEvent = parse_payload(payload)?;
            service.debug_inject(event);
            Ok(())
        }
        other => Err(unknown_command(other)),
    }
}

#[derive(serde::Deserialize, Default)]
struct VirtualIoPayload {
    /// Forget the recorded logs after answering.
    #[serde(default)]
    clear: bool,
}

#[derive(serde::Deserialize)]
struct DeviceIdPayload {
    device_id: String,
}

#[derive(serde::Deserialize)]
struct NamePayload {
    name: String,
}

#[derive(serde::Deserialize)]
struct TargetPayload {
    target: String,
}

#[derive(serde::Deserialize)]
struct RolePayload {
    role: flow_core::control::Role,
}

#[derive(serde::Deserialize)]
struct LayoutPayload {
    layout: flow_core::control::layout::ScreenLayout,
}

#[derive(serde::Deserialize)]
struct OsPayload {
    os: flow_core::device::HostOs,
}

#[derive(serde::Deserialize)]
struct RemapRulesPayload {
    rules: Vec<flow_core::keymap::remap::RemapRule>,
}

#[derive(serde::Deserialize)]
struct CandidateIdPayload {
    candidate_id: String,
}

#[derive(serde::Deserialize)]
struct RespondToPairingRequestPayload {
    request_id: String,
    decision: flow_core::pairing::PairingDecision,
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
        DaemonService::new_seeded_for_test(storage).await
    }

    #[tokio::test(start_paused = true)]
    async fn switch_to_local_round_trips_through_dispatch() {
        let service = service().await;
        let req = IpcRequest {
            id: "req-0".to_string(),
            command: "switch_to".to_string(),
            payload: json!({ "target": "local" }),
        };
        assert_eq!(
            dispatch(&service, req).await,
            IpcResponse::Ack {
                id: "req-0".to_string(),
                ok: true
            }
        );
    }

    #[tokio::test]
    async fn get_keymap_answers_with_the_keymap_file() {
        let service = service().await;
        let req = IpcRequest {
            id: "req-km".to_string(),
            command: "get_keymap".to_string(),
            payload: json!({ "os": "windows" }),
        };
        match dispatch(&service, req).await {
            IpcResponse::Data { result, ok, .. } => {
                assert!(ok);
                assert_eq!(result["os"], "windows");
            }
            other => panic!("expected data, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn switch_active_device_round_trips_through_dispatch() {
        let service = service().await;
        let _link = service.connect_peer_for_test("d2").await;
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
    async fn retry_connection_round_trips_through_dispatch() {
        use flow_core::link::DaemonLinkState;

        let service = service().await;
        service.set_link_state(DaemonLinkState::Disconnected);
        let req = IpcRequest {
            id: "req-6".to_string(),
            command: "retry_connection".to_string(),
            payload: Value::Null,
        };
        let response = dispatch(&service, req).await;
        assert_eq!(
            response,
            IpcResponse::Ack {
                id: "req-6".to_string(),
                ok: true
            }
        );
        assert_eq!(
            *service.watch_link_state().borrow(),
            DaemonLinkState::Connecting
        );
    }

    #[tokio::test]
    async fn retry_connection_while_already_connected_is_rejected() {
        let service = service().await;
        let req = IpcRequest {
            id: "req-7".to_string(),
            command: "retry_connection".to_string(),
            payload: Value::Null,
        };
        let response = dispatch(&service, req).await;
        match response {
            IpcResponse::Err { ok, error, .. } => {
                assert!(!ok);
                assert_eq!(error.code, "link_not_recoverable");
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

    #[tokio::test]
    async fn debug_inject_input_is_an_unknown_command_without_test_hooks() {
        let service = service().await; // test hooks off by default
        let resp = dispatch(
            &service,
            IpcRequest {
                id: "req-di-1".to_string(),
                command: "debug_inject_input".to_string(),
                payload: json!({ "Keyboard": { "KeyDown": { "key": "H", "modifiers": [], "timestamp_ms": 0 } } }),
            },
        )
        .await;
        match resp {
            IpcResponse::Err { error, .. } => assert_eq!(error.code, "unknown_command"),
            other => panic!("expected unknown_command, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn debug_inject_input_acks_and_reaches_the_active_peer_with_test_hooks() {
        use flow_core::control::Role;
        use flow_core::device::{HostOs, ScreenSize};

        let storage = Storage::open_in_memory().await.expect("open db");
        let service = DaemonService::new_seeded_for_test(storage)
            .await
            .with_test_hooks(true);
        let d2 = flow_core::device::DeviceId("d2".to_string());
        let mut link = service.register_link(&d2).expect("slot");
        let peer = crate::service::PeerInfo {
            role: Role::Controlled,
            os: HostOs::Windows,
            screen: ScreenSize {
                width: 100,
                height: 100,
            },
        };
        service.link_up(&d2, &peer, link.outbox.clone()).await;
        service.switch_to("d2").await.expect("switch");

        let resp = dispatch(
            &service,
            IpcRequest {
                id: "req-di-2".to_string(),
                command: "debug_inject_input".to_string(),
                payload: json!({ "Keyboard": { "KeyDown": { "key": "H", "modifiers": [], "timestamp_ms": 7 } } }),
            },
        )
        .await;
        assert_eq!(
            resp,
            IpcResponse::Ack {
                id: "req-di-2".to_string(),
                ok: true
            }
        );

        // Skip the arrangement sent at link-up and the Enter of the switch.
        let mut queued = std::iter::from_fn(|| link.inbox.try_recv().ok())
            .filter(|m| !matches!(m, crate::router::LinkOut::Arrangement { .. }));
        let _enter = queued.next().expect("enter queued");
        let Some(crate::router::LinkOut::Input(got)) = queued.next() else {
            panic!("expected the injected event to be forwarded");
        };
        assert_eq!(
            got,
            flow_core::protocol::InputEvent::Keyboard(
                flow_core::protocol::KeyboardEvent::KeyDown {
                    key: "H".to_string(),
                    modifiers: vec![],
                    timestamp_ms: 7,
                }
            )
        );
    }

    #[tokio::test]
    async fn support_commands_answer_with_a_path_or_url() {
        let service = service().await;
        let err = run_command(&service, "export_diagnostics", Value::Null)
            .await
            .unwrap_err();
        assert_eq!(
            err.code, "internal_error",
            "no export folder in a test service"
        );
        let err = run_command(
            &service,
            "open_permission_settings",
            json!({ "name": "Nope" }),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, "permission_not_found");
        let err = run_command(&service, "cancel_reconnect", Value::Null)
            .await
            .unwrap_err();
        assert_eq!(err.code, "not_reconnecting");
        assert_eq!(
            run_command(&service, RESYNC_COMMAND, Value::Null).await,
            Ok(None)
        );
    }

    #[tokio::test]
    async fn devices_say_whether_they_are_this_computer() {
        let service = service().await;
        let devices = serde_json::to_value(&*service.watch_devices().borrow()).unwrap();
        let local: Vec<_> = devices
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["is_local"] == true)
            .collect();
        assert_eq!(local.len(), 1);
        assert_eq!(local[0]["id"], "d1");
    }

    #[tokio::test]
    async fn respond_to_pairing_request_with_unknown_id_errs_with_contract_code() {
        let service = service().await;
        let resp = dispatch(
            &service,
            IpcRequest {
                id: "req-1".to_string(),
                command: "respond_to_pairing_request".to_string(),
                payload: json!({ "request_id": "ipr-x", "decision": "accept" }),
            },
        )
        .await;
        match resp {
            IpcResponse::Err { error, .. } => {
                assert_eq!(error.code, "pairing_request_not_found");
            }
            other => panic!("expected Err, got {other:?}"),
        }
    }
}
