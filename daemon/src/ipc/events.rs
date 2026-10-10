//! The engine's state as an event stream, shared by every transport.
//!
//! [`StateStream`] subscribes once to each of the service's watch channels
//! and yields `{ event, payload }` pairs: first the full snapshot
//! ([`StateStream::snapshot`], one event per channel in the fixed contract
//! order), then every later change ([`StateStream::next`]). The WebSocket
//! server and the desktop app's Tauri bridge both consume it, so the list
//! of events exists in exactly one place.

use flow_core::ipc::IpcResponse;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::watch;

use crate::service::DaemonService;

/// A client asks for the whole state again with this command
/// (`daemon-ipc.md`). Transports intercept it (it needs their sink); the
/// dispatcher acks it so it is never an unknown command.
pub const RESYNC_COMMAND: &str = "resync";

/// One state push: the contract's event name and its JSON payload.
#[derive(Debug, Clone, PartialEq)]
pub struct StateEvent {
    pub name: &'static str,
    pub payload: Value,
}

impl StateEvent {
    fn of<T: Serialize>(name: &'static str, value: &T) -> Self {
        Self {
            name,
            payload: serde_json::to_value(value).expect("serialize event payload"),
        }
    }

    /// The wire envelope `{ event, payload }`.
    pub fn into_response(self) -> IpcResponse {
        IpcResponse::Event {
            event: self.name.to_string(),
            payload: self.payload,
        }
    }
}

/// Every watch channel of one [`DaemonService`], subscribed once.
pub struct StateStream {
    devices: watch::Receiver<Vec<flow_core::device::Device>>,
    link_state: watch::Receiver<flow_core::link::DaemonLinkState>,
    pairing_session: watch::Receiver<flow_core::pairing::PairingSession>,
    settings: watch::Receiver<flow_core::settings::FlowSettings>,
    permission: watch::Receiver<flow_core::permission::PermissionStatus>,
    incoming_request: watch::Receiver<Option<flow_core::pairing::IncomingPairingRequest>>,
    control_state: watch::Receiver<flow_core::control::ControlState>,
    remap_rules: watch::Receiver<Vec<flow_core::keymap::remap::RemapRule>>,
    layout: watch::Receiver<flow_core::control::layout::ScreenLayout>,
    diagnostics: watch::Receiver<flow_core::control::Diagnostics>,
}

impl StateStream {
    pub fn new(service: &DaemonService) -> Self {
        Self {
            devices: service.watch_devices(),
            link_state: service.watch_link_state(),
            pairing_session: service.watch_pairing_session(),
            settings: service.watch_settings(),
            permission: service.watch_permission(),
            incoming_request: service.watch_incoming_request(),
            control_state: service.watch_control_state(),
            remap_rules: service.watch_remap_rules(),
            layout: service.watch_screen_layout(),
            diagnostics: service.watch_diagnostics(),
        }
    }

    /// The current value of every channel, in contract order, marking each
    /// as seen so [`Self::next`] only reports later changes. Safe to call
    /// again at any time (that is `resync`).
    pub fn snapshot(&mut self, service: &DaemonService) -> Vec<StateEvent> {
        // Each value is cloned out of its `watch::Ref` guard (not `Send`)
        // before this function returns, never across an await.
        let events = vec![
            StateEvent::of("devices_changed", &*self.devices.borrow_and_update()),
            StateEvent::of("link_state_changed", &*self.link_state.borrow_and_update()),
            StateEvent::of(
                "pairing_session_changed",
                &*self.pairing_session.borrow_and_update(),
            ),
            StateEvent::of("settings_changed", &*self.settings.borrow_and_update()),
            StateEvent::of("permission_changed", &*self.permission.borrow_and_update()),
            StateEvent::of(
                "incoming_pairing_request_changed",
                &*self.incoming_request.borrow_and_update(),
            ),
            StateEvent::of(
                "control_state_changed",
                &*self.control_state.borrow_and_update(),
            ),
            StateEvent::of(
                "remap_rules_changed",
                &*self.remap_rules.borrow_and_update(),
            ),
            StateEvent::of("screen_layout_changed", &*self.layout.borrow_and_update()),
            // Diagnostics are sampled, not stored: ask for a fresh reading.
            StateEvent::of("diagnostics_changed", &service.diagnostics()),
        ];
        self.diagnostics.borrow_and_update();
        events
    }

    /// The next change on any channel; `None` once the service is gone.
    pub async fn next(&mut self) -> Option<StateEvent> {
        tokio::select! {
            r = self.diagnostics.changed() => r.ok()
                .map(|()| StateEvent::of("diagnostics_changed", &*self.diagnostics.borrow_and_update())),
            r = self.layout.changed() => r.ok()
                .map(|()| StateEvent::of("screen_layout_changed", &*self.layout.borrow_and_update())),
            r = self.control_state.changed() => r.ok()
                .map(|()| StateEvent::of("control_state_changed", &*self.control_state.borrow_and_update())),
            r = self.remap_rules.changed() => r.ok()
                .map(|()| StateEvent::of("remap_rules_changed", &*self.remap_rules.borrow_and_update())),
            r = self.devices.changed() => r.ok()
                .map(|()| StateEvent::of("devices_changed", &*self.devices.borrow_and_update())),
            r = self.link_state.changed() => r.ok()
                .map(|()| StateEvent::of("link_state_changed", &*self.link_state.borrow_and_update())),
            r = self.pairing_session.changed() => r.ok()
                .map(|()| StateEvent::of("pairing_session_changed", &*self.pairing_session.borrow_and_update())),
            r = self.settings.changed() => r.ok()
                .map(|()| StateEvent::of("settings_changed", &*self.settings.borrow_and_update())),
            r = self.permission.changed() => r.ok()
                .map(|()| StateEvent::of("permission_changed", &*self.permission.borrow_and_update())),
            r = self.incoming_request.changed() => r.ok()
                .map(|()| StateEvent::of("incoming_pairing_request_changed", &*self.incoming_request.borrow_and_update())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    async fn service() -> DaemonService {
        let storage = Storage::open_in_memory().await.expect("open db");
        DaemonService::new_seeded_for_test(storage).await
    }

    #[tokio::test]
    async fn the_snapshot_has_all_ten_events_in_contract_order() {
        let service = service().await;
        let mut stream = StateStream::new(&service);
        let names: Vec<_> = stream.snapshot(&service).iter().map(|e| e.name).collect();
        assert_eq!(
            names,
            [
                "devices_changed",
                "link_state_changed",
                "pairing_session_changed",
                "settings_changed",
                "permission_changed",
                "incoming_pairing_request_changed",
                "control_state_changed",
                "remap_rules_changed",
                "screen_layout_changed",
                "diagnostics_changed",
            ]
        );
    }

    #[tokio::test]
    async fn changes_after_the_snapshot_are_streamed_and_a_resync_repeats_everything() {
        let service = service().await;
        let mut stream = StateStream::new(&service);
        stream.snapshot(&service);

        service.set_link_state(flow_core::link::DaemonLinkState::Disconnected);
        let event = stream.next().await.expect("an event");
        assert_eq!(event.name, "link_state_changed");
        assert_eq!(event.payload, serde_json::json!("disconnected"));

        assert_eq!(stream.snapshot(&service).len(), 10);
    }
}
