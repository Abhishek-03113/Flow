//! OS permission state: probing, publishing and requesting.

use flow_core::error::FlowError;
use flow_core::link::DaemonLinkState;
use flow_core::permission::PermissionStatus;

use super::{default_permission, DaemonService, ServiceOptions};

/// The permission state a service starts with: probed from the attached
/// platform, or the per-OS fixture when none is attached (tests).
pub(super) fn initial_permission(options: &ServiceOptions) -> PermissionStatus {
    match &options.backend {
        Some(backend) => PermissionStatus::from_items(backend.permissions()),
        None => default_permission(options.host_os),
    }
}

impl DaemonService {
    /// Re-probes the OS and publishes the result when it changed. Missing
    /// permission with no live link is `permission_required`; once granted
    /// that state clears. Does nothing without an attached platform.
    pub async fn refresh_permissions(&self) {
        let Some(backend) = &self.backend else { return };
        let status = PermissionStatus::from_items(backend.permissions());
        let changed = {
            let mut state = self.state.write().await;
            let changed = state.permission != status;
            state.permission = status.clone();
            changed
        };
        if changed {
            tracing::info!(granted = status.granted, "input permission changed");
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

    /// Shows the real OS permission prompt (where the OS has one) for
    /// everything still missing, then re-probes. With no platform attached
    /// (tests) it simply marks the fixture granted.
    #[tracing::instrument(skip(self))]
    pub async fn request_permission(&self) -> Result<(), FlowError> {
        if self.state.read().await.permission.granted {
            return Err(FlowError::PermissionAlreadyGranted);
        }
        match &self.backend {
            Some(backend) => {
                backend.request_permission(None);
                self.refresh_permissions().await;
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
        fn request_permission(&self, _name: Option<&str>) {
            *self.prompts.lock().unwrap() += 1;
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
