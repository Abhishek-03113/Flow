//! OS permission state: probing, publishing and requesting.

use flow_core::error::FlowError;
use flow_core::link::DaemonLinkState;
use flow_core::permission::PermissionStatus;
use flow_platform::permissions::SetupError;

use super::{default_permission, DaemonService, ServiceOptions};

/// The permission state a service starts with: probed from the attached
/// platform, or the per-OS fixture when none is attached (tests).
pub(super) fn initial_permission(options: &ServiceOptions) -> PermissionStatus {
    match &options.backend {
        Some(backend) => PermissionStatus::from_items(backend.permissions()),
        None => default_permission(options.host_os),
    }
}

/// Consecutive announce ticks on which every send failed before the
/// engine reports the local network as unreachable. One bad tick is a
/// network change in progress; three (15 s) is a state.
pub const LOCAL_NETWORK_FAILED_TICKS: u32 = 3;

impl DaemonService {
    /// The engine reports each announce tick: `all_failed` when not one
    /// destination accepted the packet (macOS Local Network privacy denied
    /// fails every send with "no route to host"). Re-probes permissions
    /// when the verdict flips.
    pub async fn note_announce_result(&self, all_failed: bool) {
        let before = self.local_network_blocked();
        if all_failed {
            self.announce_failures
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else {
            self.announce_failures
                .store(0, std::sync::atomic::Ordering::Relaxed);
        }
        let after = self.local_network_blocked();
        if before != after {
            if after {
                crate::logging::product::error(
                    "discovery",
                    &"every announce has failed to leave this computer (local network blocked or unavailable)",
                );
            } else {
                tracing::info!(target: "flow", "[NET] announces reach the local network again");
            }
            self.refresh_permissions().await;
        }
    }

    fn local_network_blocked(&self) -> bool {
        self.announce_failures
            .load(std::sync::atomic::Ordering::Relaxed)
            >= LOCAL_NETWORK_FAILED_TICKS
    }

    /// Re-probes the OS and publishes the result when it changed. Missing
    /// permission with no live link is `permission_required`; once granted
    /// that state clears. Does nothing without an attached platform.
    pub async fn refresh_permissions(&self) {
        let Some(backend) = &self.backend else { return };
        let mut items = backend.permissions();
        if self.local_network_blocked() {
            items.push(flow_platform::permissions::local_network_item());
        }
        let mut status = PermissionStatus::from_items(items);
        status.one_click_setup = !status.granted && backend.permission_one_click_setup();
        let changed = {
            let mut state = self.state.write().await;
            // Only an input grant needs a fresh process; the local network
            // item clears by itself.
            if status
                .items
                .iter()
                .any(|i| !i.granted && i.name != flow_platform::permissions::LOCAL_NETWORK)
            {
                state.permission_missing_seen = true;
            }
            // Granted while running, on an OS that applies it only to a
            // fresh process: say so, instead of capture silently failing.
            status.restart_required = status.granted
                && state.permission_missing_seen
                && backend.permission_grant_needs_restart();
            let changed = state.permission != status;
            state.permission = status.clone();
            changed
        };
        if changed {
            let missing: Vec<&str> = status
                .items
                .iter()
                .filter(|i| !i.granted)
                .map(|i| i.name.as_str())
                .collect();
            tracing::info!(
                target: "flow",
                "[PERMISSION] granted={} missing=[{}] restart_required={}",
                status.granted,
                missing.join(", "),
                status.restart_required
            );
            self.permission_tx.send_replace(status.clone());
        }
        self.gate_link_on_permission(status.granted);
    }

    fn gate_link_on_permission(&self, granted: bool) {
        let current = *self.link_state_tx.borrow();
        let any_live = self
            .links
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .any(|link| link.peer_role.is_some());
        if !granted && !any_live && current != DaemonLinkState::PermissionRequired {
            self.set_link_state(DaemonLinkState::PermissionRequired);
        } else if granted && current == DaemonLinkState::PermissionRequired {
            self.set_link_state(if any_live {
                DaemonLinkState::Connected
            } else {
                DaemonLinkState::Disconnected
            });
        }
    }

    /// Gets everything still missing granted (macOS: the OS prompts; Linux:
    /// the one-time system setup behind one administrator prompt), then
    /// re-probes. Each way it can fail is its own error code, and logged.
    /// With no platform attached (tests) it simply marks the fixture granted.
    #[tracing::instrument(skip(self))]
    pub async fn request_permission(&self) -> Result<(), FlowError> {
        if self.state.read().await.permission.granted {
            return Err(FlowError::PermissionAlreadyGranted);
        }
        match &self.backend {
            Some(backend) => {
                let backend = std::sync::Arc::clone(backend);
                // May wait on a password prompt: keep it off the runtime.
                let result = tokio::task::spawn_blocking(move || backend.request_permission(None))
                    .await
                    .unwrap_or_else(|err| {
                        Err(SetupError::Failed(format!("the setup task died: {err}")))
                    });
                self.refresh_permissions().await;
                if let Err(err) = result {
                    crate::logging::product::error("permission setup", &err.to_string());
                    return Err(match err {
                        SetupError::Cancelled => FlowError::PermissionSetupCancelled,
                        SetupError::Unavailable(why) => FlowError::PermissionSetupUnavailable(why),
                        SetupError::Failed(why) => FlowError::PermissionSetupFailed(why),
                    });
                }
            }
            None => {
                let permission = {
                    let mut state = self.state.write().await;
                    state.permission.granted = true;
                    for item in &mut state.permission.items {
                        item.granted = true;
                        item.how_to_fix = None;
                    }
                    state.permission.clone()
                };
                self.permission_tx.send_replace(permission);
            }
        }
        Ok(())
    }

    /// The OS settings page where `name` is granted: a URL for the shell to
    /// open, or `None` when this OS has no such page (the item's
    /// `how_to_fix` text applies).
    pub async fn permission_settings_url(&self, name: &str) -> Result<Option<String>, FlowError> {
        let known = self
            .state
            .read()
            .await
            .permission
            .items
            .iter()
            .any(|item| item.name == name);
        if !known {
            return Err(FlowError::PermissionNotFound(name.to_string()));
        }
        Ok(self
            .backend
            .as_ref()
            .and_then(|backend| backend.permission_settings_url(name)))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use flow_core::device::{HostOs, ScreenSize};
    use flow_core::input::CaptureSink;
    use flow_core::permission::PermissionItem;

    use super::*;
    use crate::platform_backend::{BackendError, CaptureGuard, Injector, PlatformBackend};
    use crate::storage::Storage;
    use crate::virtual_backend::VirtualBackend;

    /// A virtual machine whose OS permissions the test controls.
    struct Fake {
        inner: VirtualBackend,
        granted: Mutex<bool>,
        prompts: Mutex<u32>,
    }

    impl PlatformBackend for Fake {
        fn host_os(&self) -> HostOs {
            self.inner.host_os()
        }
        fn screen_size(&self) -> ScreenSize {
            self.inner.screen_size()
        }
        fn start_capture(&self, sink: Arc<dyn CaptureSink>) -> Result<CaptureGuard, BackendError> {
            self.inner.start_capture(sink)
        }
        fn new_injector(&self) -> Result<Box<dyn Injector>, BackendError> {
            self.inner.new_injector()
        }
        fn permissions(&self) -> Vec<PermissionItem> {
            let granted = *self.granted.lock().unwrap();
            ["Accessibility", "Input Monitoring"]
                .into_iter()
                .map(|name| PermissionItem {
                    name: name.into(),
                    granted,
                    how_to_fix: (!granted).then(|| flow_core::permission::HowToFix {
                        summary: "Turn it on.".into(),
                        steps: vec![],
                    }),
                })
                .collect()
        }
        fn request_permission(&self, _name: Option<&str>) -> Result<(), SetupError> {
            *self.prompts.lock().unwrap() += 1;
            Ok(())
        }
        fn permission_grant_needs_restart(&self) -> bool {
            true
        }
        fn permission_settings_url(&self, name: &str) -> Option<String> {
            Some(format!("settings://{name}"))
        }
    }

    async fn service_on(fake: &Arc<Fake>) -> DaemonService {
        let storage = Storage::open_in_memory().await.expect("db");
        DaemonService::new_with_options(
            storage,
            ServiceOptions {
                backend: Some(Arc::clone(fake) as Arc<dyn PlatformBackend>),
                ..ServiceOptions::default()
            },
        )
        .await
        .expect("service")
    }

    fn fake(granted: bool) -> Arc<Fake> {
        Arc::new(Fake {
            inner: VirtualBackend::new(
                HostOs::Macos,
                ScreenSize {
                    width: 100,
                    height: 100,
                },
            ),
            granted: Mutex::new(granted),
            prompts: Mutex::new(0),
        })
    }

    #[tokio::test]
    async fn announces_that_never_leave_this_computer_are_a_local_network_item() {
        let service = service_on(&fake(true)).await;
        let names = |s: &DaemonService| -> Vec<String> {
            s.watch_permission()
                .borrow()
                .items
                .iter()
                .filter(|i| !i.granted)
                .map(|i| i.name.clone())
                .collect()
        };
        // One or two bad ticks are a network change in progress.
        for _ in 1..LOCAL_NETWORK_FAILED_TICKS {
            service.note_announce_result(true).await;
        }
        assert!(names(&service).is_empty());

        service.note_announce_result(true).await;
        assert_eq!(names(&service), ["Local network"]);
        let status = service.watch_permission().borrow().clone();
        assert!(!status.granted);
        let item = status
            .items
            .iter()
            .find(|i| i.name == "Local network")
            .unwrap();
        assert!(item.how_to_fix.is_some());

        // One delivered announce clears it.
        service.note_announce_result(false).await;
        assert!(names(&service).is_empty());
        assert!(service.watch_permission().borrow().granted);
    }

    #[tokio::test]
    async fn each_missing_permission_is_reported_separately() {
        let service = service_on(&fake(false)).await;
        let status = service.watch_permission().borrow().clone();
        assert!(!status.granted);
        let names: Vec<_> = status.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["Accessibility", "Input Monitoring"]);
        assert!(status.items.iter().all(|i| i.how_to_fix.is_some()));
    }

    #[tokio::test]
    async fn requesting_shows_the_os_prompt_and_a_grant_clears_permission_required() {
        let fake = fake(false);
        let service = service_on(&fake).await;
        service.refresh_permissions().await;
        assert_eq!(
            *service.watch_link_state().borrow(),
            DaemonLinkState::PermissionRequired
        );

        service.request_permission().await.expect("request");
        assert_eq!(
            *fake.prompts.lock().unwrap(),
            1,
            "the real prompt was asked for"
        );

        *fake.granted.lock().unwrap() = true;
        service.refresh_permissions().await;
        assert!(service.watch_permission().borrow().granted);
        assert_eq!(
            *service.watch_link_state().borrow(),
            DaemonLinkState::Disconnected
        );
        assert_eq!(
            service.request_permission().await,
            Err(FlowError::PermissionAlreadyGranted)
        );
    }

    #[tokio::test]
    async fn a_grant_made_while_running_asks_for_a_restart_where_the_os_needs_one() {
        let fake = fake(false);
        let service = service_on(&fake).await;
        service.refresh_permissions().await;
        assert!(!service.watch_permission().borrow().restart_required);
        *fake.granted.lock().unwrap() = true;
        service.refresh_permissions().await;
        let status = service.watch_permission().borrow().clone();
        assert!(status.granted && status.restart_required);
    }

    #[tokio::test]
    async fn permissions_granted_from_the_start_need_no_restart() {
        let service = service_on(&fake(true)).await;
        service.refresh_permissions().await;
        assert!(!service.watch_permission().borrow().restart_required);
    }

    #[tokio::test]
    async fn settings_urls_are_resolved_per_known_item_only() {
        let service = service_on(&fake(false)).await;
        assert_eq!(
            service.permission_settings_url("Accessibility").await,
            Ok(Some("settings://Accessibility".to_string()))
        );
        assert_eq!(
            service.permission_settings_url("Bogus").await,
            Err(FlowError::PermissionNotFound("Bogus".to_string()))
        );
    }
}
