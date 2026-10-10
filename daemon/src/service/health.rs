//! Link health: when peers were last heard from, and whether the engine
//! may dial paired computers.

use std::sync::atomic::Ordering;
use std::time::Duration;

use chrono::Utc;
use flow_core::device::DeviceId;
use flow_core::error::FlowError;
use flow_core::link::DaemonLinkState;

use super::DaemonService;
use crate::storage::device_repo::DeviceRepo;

/// At most one `last_seen` update per peer in this window, so a 2 s
/// heartbeat does not become a 2 s `devices_changed` stream.
const HEARD_PUBLISH_EVERY: Duration = Duration::from_secs(5);

impl DaemonService {
    /// A peer sent us something: it is alive now. `last_seen` therefore
    /// means "last heard from", not "last switched to".
    pub async fn note_heard(&self, device_id: &DeviceId) {
        let due = {
            let mut heard = self
                .heard_published
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            match heard.get(device_id) {
                Some(at) if at.elapsed() < HEARD_PUBLISH_EVERY => false,
                _ => {
                    heard.insert(device_id.clone(), std::time::Instant::now());
                    true
                }
            }
        };
        if due {
            let now = Utc::now();
            self.update_device(device_id, |device| device.last_seen = now)
                .await;
            // Same throttle as the publish above, so a 2 s heartbeat is
            // one write per peer per 5 s, not one per heartbeat. Best
            // effort: a failed write only costs a stale "last seen" after
            // a restart.
            if let Err(err) = DeviceRepo::new(self.storage.clone())
                .set_last_seen(device_id.clone(), now)
                .await
            {
                tracing::warn!(%err, device = %device_id.0, "could not persist last_seen");
            }
        }
    }

    /// Whether discovery may dial paired computers right now: not while the
    /// user cancelled reconnecting, and only if `auto_connect_paired_devices`.
    pub(super) fn may_dial(&self) -> bool {
        self.settings_tx.borrow().auto_connect_paired_devices
            && !self.redial_paused.load(Ordering::Relaxed)
    }

    /// Stops redialling after a lost link until the user retries
    /// (`retry_connection`) or a computer connects to this one.
    #[tracing::instrument(skip(self))]
    pub async fn cancel_reconnect(&self) -> Result<(), FlowError> {
        let current = *self.link_state_tx.borrow();
        if current != DaemonLinkState::Reconnecting {
            return Err(FlowError::NotReconnecting(current));
        }
        self.redial_paused.store(true, Ordering::Relaxed);
        self.set_link_state(DaemonLinkState::Disconnected);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use flow_core::settings::SettingsPatch;

    use super::*;
    use crate::storage::device_repo::DeviceRepo;
    use crate::storage::Storage;

    async fn service() -> DaemonService {
        let storage = Storage::open_in_memory().await.expect("db");
        DaemonService::new_seeded_for_test(storage).await
    }

    #[tokio::test]
    async fn heard_peers_have_last_seen_persisted_at_most_once_per_window() {
        let storage = Storage::open_in_memory().await.expect("db");
        let service = DaemonService::new_seeded_for_test(storage.clone()).await;
        let repo = DeviceRepo::new(storage);
        let id = DeviceId("d2".to_string());
        let stored = |repo: DeviceRepo, id: DeviceId| async move {
            repo.find_by_id(id)
                .await
                .expect("query")
                .expect("d2 is stored")
                .device
                .last_seen
        };

        let before = stored(repo.clone(), id.clone()).await;
        service.note_heard(&id).await;
        let after = stored(repo.clone(), id.clone()).await;
        assert!(after > before, "heard peer must persist a newer last_seen");
        assert!(
            (Utc::now() - after).num_seconds() <= 5,
            "persisted time is the time it was heard"
        );

        // Inside the throttle window a second heartbeat writes nothing.
        let sentinel = before;
        repo.set_last_seen(id.clone(), sentinel)
            .await
            .expect("reset");
        service.note_heard(&id).await;
        assert_eq!(stored(repo, id).await, sentinel);
    }

    #[tokio::test]
    async fn link_states_follow_pairing_loss_cancel_and_retry() {
        let service = service().await;
        let id = DeviceId("d2".to_string());
        let link = service.connect_peer_for_test("d2").await;
        assert_eq!(
            *service.watch_link_state().borrow(),
            DaemonLinkState::Connected
        );

        drop(link);
        service.link_down(&id, None).await;
        assert_eq!(
            *service.watch_link_state().borrow(),
            DaemonLinkState::Reconnecting
        );
        assert!(service.may_dial());

        service.cancel_reconnect().await.expect("cancel");
        assert_eq!(
            *service.watch_link_state().borrow(),
            DaemonLinkState::Disconnected
        );
        assert!(!service.may_dial(), "no redialling after Cancel");
        assert_eq!(
            service.cancel_reconnect().await,
            Err(FlowError::NotReconnecting(DaemonLinkState::Disconnected))
        );

        service.retry_connection().await.expect("retry");
        assert_eq!(
            *service.watch_link_state().borrow(),
            DaemonLinkState::Connecting
        );
        assert!(service.may_dial(), "retry resumes redialling");
    }

    #[tokio::test]
    async fn a_role_conflict_is_an_error_not_a_reconnect_and_is_not_redialled() {
        use flow_core::control::Role;
        use flow_core::device::{HostOs, ScreenSize};

        let service = service().await;
        let id = DeviceId("d2".to_string());
        let link = service.connect_peer_for_test("d2").await;
        drop(link);
        // The peer came back as a controller too and was refused.
        let peer = super::super::PeerInfo {
            role: Role::Controller,
            os: HostOs::Windows,
            screen: ScreenSize {
                width: 1920,
                height: 1080,
            },
        };
        service.note_refused_peer(&id, &peer).await;
        service.link_down(&id, Some("role_conflict")).await;

        assert_eq!(*service.watch_link_state().borrow(), DaemonLinkState::Error);
        let device = service
            .watch_devices()
            .borrow()
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .expect("device");
        assert_eq!(device.role, Some(Role::Controller), "the role it announced");
        assert_eq!(device.link_error.as_deref(), Some("role_conflict"));
        let refused = |s: &DaemonService| s.refused_peers.lock().unwrap().contains(&id);
        assert!(refused(&service), "not redialled");

        service.set_role(Role::Controlled).await.expect("role");
        assert!(!refused(&service), "a role change dials again");
    }

    #[tokio::test]
    async fn auto_connect_paired_devices_gates_dialling() {
        let service = service().await;
        assert!(service.may_dial());
        service
            .update_settings(SettingsPatch {
                auto_connect_paired_devices: Some(false),
                ..Default::default()
            })
            .await
            .expect("update");
        assert!(!service.may_dial());
    }

    #[tokio::test(start_paused = true)]
    async fn last_seen_moves_when_the_peer_is_heard_not_when_it_is_switched_to() {
        let service = service().await;
        let id = DeviceId("d2".to_string());
        let _link = service.connect_peer_for_test("d2").await;
        let seen = |s: &DaemonService| {
            s.watch_devices()
                .borrow()
                .iter()
                .find(|d| d.id == id)
                .map(|d| d.last_seen)
                .expect("device")
        };

        let at_link_up = seen(&service);
        std::thread::sleep(std::time::Duration::from_millis(5));
        service.switch_to("d2").await.expect("switch");
        tokio::task::yield_now().await;
        assert_eq!(seen(&service), at_link_up, "switching is not hearing");

        tokio::time::advance(HEARD_PUBLISH_EVERY + Duration::from_secs(1)).await;
        std::thread::sleep(std::time::Duration::from_millis(5));
        service.note_heard(&id).await;
        assert!(seen(&service) > at_link_up);
    }
}
