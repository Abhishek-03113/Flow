//! Direct reconnection to paired computers (`docs/redesign/flows-v3.md`
//! §3.5). Discovery is a hint: once two computers are paired, each one
//! remembers where it last reached the other and dials that address
//! itself, so they reconnect on networks that never carry broadcast.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use flow_core::channel::PEER_PORT;
use flow_core::device::DeviceId;

use super::DaemonService;
use crate::storage::device_repo::DeviceRepo;

/// Where each paired computer was last reached, shared by the service and
/// the engine's reconnect supervisor.
pub(super) type PeerAddresses = Arc<std::sync::Mutex<HashMap<DeviceId, SocketAddr>>>;

impl DaemonService {
    /// Loads the stored addresses of paired computers (startup).
    pub(super) async fn load_peer_addresses(&self) {
        match DeviceRepo::new(self.storage.clone()).last_addresses().await {
            Ok(rows) => {
                let mut map = self
                    .peer_addresses
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                map.extend(rows);
            }
            Err(err) => tracing::warn!(%err, "could not load paired computers' addresses"),
        }
    }

    /// Records that `device_id`'s peer listener answered at `address`.
    /// Persisted only when it changed.
    pub async fn note_peer_address(&self, device_id: &DeviceId, address: SocketAddr) {
        let changed = {
            let mut map = self
                .peer_addresses
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            map.insert(device_id.clone(), address) != Some(address)
        };
        if !changed {
            return;
        }
        tracing::info!(target: "flow", "[NET] {} is at {address}", short(device_id));
        if let Err(err) = DeviceRepo::new(self.storage.clone())
            .set_last_address(device_id.clone(), address)
            .await
        {
            tracing::warn!(%err, "could not persist a paired computer's address");
        }
    }

    /// A trusted computer connected to this one from `from` (a source port,
    /// not its listener). Its listener is most likely on the well-known
    /// port at that IP; use that only when nothing better is known.
    pub async fn note_inbound_peer(&self, device_id: &DeviceId, from: Option<SocketAddr>) {
        let Some(from) = from else { return };
        let known = self
            .peer_addresses
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(device_id)
            .copied();
        match known {
            Some(addr) if addr.ip() == from.ip() => {}
            Some(addr) => {
                // Same computer, new IP (DHCP, another network): keep the
                // port it listened on before.
                self.note_peer_address(device_id, SocketAddr::new(from.ip(), addr.port()))
                    .await;
            }
            None => {
                self.note_peer_address(device_id, SocketAddr::new(from.ip(), PEER_PORT))
                    .await
            }
        }
    }

    pub(super) fn forget_peer_address(&self, device_id: &DeviceId) {
        self.peer_addresses
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(device_id);
    }

    /// Paired computers to dial now: known address, still paired, no live
    /// link, not refused for their role, and redialling allowed.
    pub async fn redial_targets(&self) -> Vec<(DeviceId, SocketAddr)> {
        if !self.may_dial() {
            return Vec::new();
        }
        let addresses: Vec<(DeviceId, SocketAddr)> = self
            .peer_addresses
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(id, addr)| (id.clone(), *addr))
            .collect();
        let state = self.state.read().await;
        let refused = self.refused_peers.lock().unwrap_or_else(|e| e.into_inner());
        addresses
            .into_iter()
            .filter(|(id, _)| state.devices.contains_key(id))
            .filter(|(id, _)| !refused.contains(id))
            .filter(|(id, _)| !self.has_link(id))
            .collect()
    }

    /// Signalled when every paired computer should be dialled at once
    /// (Retry pressed).
    pub fn redial_requests(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.redial_now)
    }
}

fn short(device_id: &DeviceId) -> &str {
    let id = device_id.0.strip_prefix("pk:").unwrap_or(&device_id.0);
    &id[..id.len().min(8)]
}
