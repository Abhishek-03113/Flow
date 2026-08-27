//! TCP/Wi-Fi peer discovery (`daemon/todos.json` G3): a lightweight UDP
//! broadcast announce/listen loop so `start_pairing`'s candidate list
//! can eventually be populated by real TCP-reachable peers, not just the
//! mock-parity 2 hardcoded ones.
//!
//! `vision.md` §22 Phase 1 (Connectivity): "Establish communication
//! between two machines... Validate: Device discovery, Connection,
//! Disconnect, Reconnect, Basic messages."

use std::net::{Ipv4Addr, SocketAddr};

use flow_core::channel::ChannelAddress;
use flow_core::device::HostOs;
use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use super::DiscoveredPeer;

/// The well-known UDP port every Flow daemon's discovery listener binds
/// to in real deployment. Distinct from `flow_core::ipc::IPC_PORT`
/// (local Flutter<->daemon) and from wherever a peer's own `TcpChannel`
/// listener ends up bound (advertised inside the announce packet
/// itself, since it can vary per daemon).
pub const DISCOVERY_PORT: u16 = 47824;

/// The wire shape of one announce packet.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Announce {
    name: String,
    os: HostOs,
    /// The port this peer's own `TcpChannel` listener is bound to —
    /// separate from `DISCOVERY_PORT`, which only ever carries announces.
    channel_port: u16,
    /// Who sent this, so a receiver can recognize its own announce
    /// echoing back. A real broadcast to `255.255.255.255` is delivered
    /// to the sending host too, so without this every daemon
    /// "discovers" itself and starts handshaking against its own peer
    /// listener every announce interval. Not a security boundary —
    /// anything on the network can put any id here; it exists purely to
    /// filter a loopback echo, and is never used to decide trust (that
    /// stays with the Noise-proven identity key).
    instance_id: String,
}

/// Broadcasts this daemon's presence and listens for others' announces.
pub struct DiscoveryService {
    socket: UdpSocket,
    name: String,
    os: HostOs,
    channel_port: u16,
    instance_id: String,
}

impl DiscoveryService {
    /// Binds the discovery listener on `listen_port` (in real
    /// deployment, always [`DISCOVERY_PORT`]; tests use distinct
    /// per-instance ports since two same-host instances can't share
    /// one). Binds `0.0.0.0` so a broadcast arriving on any interface is
    /// received regardless of which one it came in on.
    /// `instance_id` identifies this daemon process for the sole purpose
    /// of ignoring its own broadcast echo — see [`Announce::instance_id`].
    /// Callers should pass something stable for the process's lifetime
    /// and distinct per daemon; `main.rs` uses this daemon's own
    /// identity public key, which it already has and which is unique by
    /// construction.
    pub async fn bind(
        listen_port: u16,
        name: String,
        os: HostOs,
        channel_port: u16,
        instance_id: String,
    ) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(("0.0.0.0", listen_port)).await?;
        socket.set_broadcast(true)?;
        Ok(Self {
            socket,
            name,
            os,
            channel_port,
            instance_id,
        })
    }

    /// The port this service actually bound to (useful when `bind` was
    /// called with port `0` for an OS-assigned ephemeral port, as in
    /// this module's own tests).
    pub fn local_port(&self) -> std::io::Result<u16> {
        self.socket.local_addr().map(|addr| addr.port())
    }

    /// The limited broadcast address: `255.255.255.255:DISCOVERY_PORT`.
    /// Kept as the fallback [`broadcast_destinations`] always includes,
    /// but not relied on alone in real use — see that function's doc
    /// comment for why.
    pub fn broadcast_destination() -> SocketAddr {
        SocketAddr::new(Ipv4Addr::BROADCAST.into(), DISCOVERY_PORT)
    }

    /// Every destination this daemon's periodic announce should be sent
    /// to: each active IPv4 interface's own subnet-directed broadcast
    /// address (e.g. `192.168.1.255` for a host on `192.168.1.0/24`),
    /// plus the limited broadcast [`broadcast_destination`] as a
    /// fallback.
    ///
    /// A single send to `255.255.255.255` is not enough on a real,
    /// multi-homed machine — exactly the case of two daemons "each
    /// running and listening but never seeing each other" that this
    /// function exists to fix. Sending to the limited broadcast address
    /// from a socket bound to `0.0.0.0` leaves the OS to pick *one*
    /// outgoing interface via the routing table, same as any other
    /// destination; on a dev machine with a VPN client, Docker Desktop,
    /// or Hyper-V/WSL installed (all of which add their own virtual
    /// adapter with a route of their own) that's frequently not the real
    /// LAN adapter the other daemon is actually reachable on, so the
    /// announce silently never reaches the LAN segment the peer is on —
    /// no error, no log, both sides just never discover each other.
    /// Sending each interface's own subnet broadcast instead forces the
    /// packet out over every real link explicitly, sidestepping the
    /// routing-table guess entirely. Interface enumeration failing (or
    /// finding nothing) still leaves the limited-broadcast fallback in
    /// the list, so single-NIC hosts and any environment enumeration
    /// doesn't work in are unaffected.
    pub fn broadcast_destinations() -> Vec<SocketAddr> {
        let mut destinations = Vec::new();
        if let Ok(interfaces) = if_addrs::get_if_addrs() {
            for interface in interfaces {
                if interface.is_loopback() {
                    continue;
                }
                if let if_addrs::IfAddr::V4(v4) = interface.addr {
                    if let Some(broadcast) = v4.broadcast {
                        destinations.push(SocketAddr::new(broadcast.into(), DISCOVERY_PORT));
                    }
                }
            }
        }
        let fallback = Self::broadcast_destination();
        if !destinations.contains(&fallback) {
            destinations.push(fallback);
        }
        destinations
    }

    /// Sends one announce packet to `destination`. Real use always
    /// targets [`Self::broadcast_destination`]; a caller (this module's
    /// own tests, or a future unicast re-announce) may target a
    /// specific peer address instead.
    pub async fn announce_to(&self, destination: SocketAddr) -> std::io::Result<()> {
        let packet = Announce {
            name: self.name.clone(),
            os: self.os,
            channel_port: self.channel_port,
            instance_id: self.instance_id.clone(),
        };
        let bytes = serde_json::to_vec(&packet).expect("serialize announce");
        self.socket.send_to(&bytes, destination).await?;
        Ok(())
    }

    /// Receives and parses one announce, pairing the sender's IP with
    /// the advertised `channel_port` to build its reachable
    /// `ChannelAddress`. A malformed or foreign packet (this UDP port
    /// could receive traffic that isn't a Flow announce at all), or this
    /// daemon's own announce echoing back off the broadcast address,
    /// yields `Ok(None)` rather than an error — the caller just waits
    /// for the next packet.
    pub async fn recv_one(&self) -> std::io::Result<Option<DiscoveredPeer>> {
        let mut buf = [0u8; 512];
        let (len, sender) = self.socket.recv_from(&mut buf).await?;
        let Ok(announce) = serde_json::from_slice::<Announce>(&buf[..len]) else {
            return Ok(None);
        };
        if announce.instance_id == self.instance_id {
            return Ok(None);
        }
        let tcp_addr = SocketAddr::new(sender.ip(), announce.channel_port);
        Ok(Some(DiscoveredPeer {
            name: announce.name,
            os: announce.os,
            address: ChannelAddress::Tcp(tcp_addr),
        }))
    }

    /// Spawns a background loop that receives announces indefinitely,
    /// forwarding each successfully-parsed one on the returned channel.
    /// Doesn't itself send periodic announces — deciding *when* this
    /// daemon should announce (e.g. only while `start_pairing`'s
    /// `Searching` stage is active) is track G7's job, not this module's.
    pub fn spawn_listener(self) -> mpsc::UnboundedReceiver<DiscoveredPeer> {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            loop {
                match self.recv_one().await {
                    Ok(Some(peer)) => {
                        if tx.send(peer).is_err() {
                            break;
                        }
                    }
                    Ok(None) => continue,
                    Err(_) => break,
                }
            }
        });
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `broadcast_destinations` must never come back empty — even on a
    /// host where interface enumeration fails or finds nothing routable,
    /// the limited-broadcast fallback keeps single-NIC hosts working, and
    /// it must always be present regardless of what real interfaces this
    /// test-running machine happens to have.
    #[test]
    fn broadcast_destinations_always_includes_the_limited_broadcast_fallback() {
        let destinations = DiscoveryService::broadcast_destinations();
        assert!(!destinations.is_empty());
        assert!(destinations.contains(&DiscoveryService::broadcast_destination()));
    }

    /// A loopback-only send target for a service bound with `bind(0, ...)`
    /// — `local_addr()` on a `0.0.0.0`-bound socket reports `0.0.0.0`
    /// itself, which isn't a valid *send* destination, so the test talks
    /// to each instance via `127.0.0.1:<its actual bound port>` instead.
    fn loopback(service: &DiscoveryService) -> SocketAddr {
        SocketAddr::new(Ipv4Addr::LOCALHOST.into(), service.local_port().unwrap())
    }

    #[tokio::test]
    async fn two_instances_discover_each_other_via_loopback_announces() {
        let a = DiscoveryService::bind(
            0,
            "Device A".to_string(),
            HostOs::Linux,
            47900,
            "id-a".to_string(),
        )
        .await
        .expect("bind a");
        let b = DiscoveryService::bind(
            0,
            "Device B".to_string(),
            HostOs::Macos,
            47901,
            "id-b".to_string(),
        )
        .await
        .expect("bind b");
        let (a_addr, b_addr) = (loopback(&a), loopback(&b));

        a.announce_to(b_addr).await.expect("a announces to b");
        b.announce_to(a_addr).await.expect("b announces to a");

        let discovered_by_b = b.recv_one().await.expect("recv").expect("parsed");
        assert_eq!(discovered_by_b.name, "Device A");
        assert_eq!(discovered_by_b.os, HostOs::Linux);
        assert_eq!(
            discovered_by_b.address,
            ChannelAddress::Tcp(SocketAddr::new(a_addr.ip(), 47900))
        );

        let discovered_by_a = a.recv_one().await.expect("recv").expect("parsed");
        assert_eq!(discovered_by_a.name, "Device B");
        assert_eq!(discovered_by_a.os, HostOs::Macos);
        assert_eq!(
            discovered_by_a.address,
            ChannelAddress::Tcp(SocketAddr::new(b_addr.ip(), 47901))
        );
    }

    #[tokio::test]
    async fn spawn_listener_forwards_announces_on_its_channel() {
        let a = DiscoveryService::bind(
            0,
            "Device A".to_string(),
            HostOs::Linux,
            47900,
            "id-a".to_string(),
        )
        .await
        .expect("bind a");
        let b = DiscoveryService::bind(
            0,
            "Device B".to_string(),
            HostOs::Windows,
            47901,
            "id-b".to_string(),
        )
        .await
        .expect("bind b");
        let (a_addr, b_addr) = (loopback(&a), loopback(&b));

        let mut discoveries = b.spawn_listener();
        a.announce_to(b_addr).await.expect("a announces to b");

        let discovered = discoveries.recv().await.expect("channel open");
        assert_eq!(discovered.name, "Device A");
        assert_eq!(
            discovered.address,
            ChannelAddress::Tcp(SocketAddr::new(a_addr.ip(), 47900))
        );
    }

    #[tokio::test]
    async fn a_non_announce_udp_packet_is_skipped_not_treated_as_an_error() {
        let listener = DiscoveryService::bind(
            0,
            "Device A".to_string(),
            HostOs::Linux,
            47900,
            "id-a".to_string(),
        )
        .await
        .expect("bind listener");
        let listener_addr = loopback(&listener);

        let sender = UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        sender
            .send_to(b"not json", listener_addr)
            .await
            .expect("send garbage");

        assert_eq!(listener.recv_one().await.expect("recv"), None);
    }

    /// A real broadcast to `255.255.255.255` is delivered back to the
    /// sending host, so a daemon receives its own announce. It must not
    /// treat itself as a discovered peer — otherwise it registers a
    /// pairing candidate for itself and dials its own peer listener on
    /// every announce interval.
    #[tokio::test]
    async fn a_daemons_own_announce_echoing_back_is_not_reported_as_a_peer() {
        let service = DiscoveryService::bind(
            0,
            "Device A".to_string(),
            HostOs::Linux,
            47900,
            "id-a".to_string(),
        )
        .await
        .expect("bind service");
        let own_addr = loopback(&service);

        service
            .announce_to(own_addr)
            .await
            .expect("announce to itself, as a broadcast echo would");

        assert_eq!(service.recv_one().await.expect("recv"), None);
    }

    /// The negative control for the check above: an announce carrying a
    /// *different* instance id is still reported, proving the filter
    /// keys on identity rather than suppressing loopback traffic
    /// wholesale (every test in this module talks over loopback).
    #[tokio::test]
    async fn an_announce_from_a_different_instance_over_loopback_is_still_reported() {
        let listener = DiscoveryService::bind(
            0,
            "Device A".to_string(),
            HostOs::Linux,
            47900,
            "id-a".to_string(),
        )
        .await
        .expect("bind listener");
        let sender = DiscoveryService::bind(
            0,
            "Device B".to_string(),
            HostOs::Linux,
            47901,
            "id-b".to_string(),
        )
        .await
        .expect("bind sender");

        sender
            .announce_to(loopback(&listener))
            .await
            .expect("announce");

        let discovered = listener.recv_one().await.expect("recv").expect("parsed");
        assert_eq!(discovered.name, "Device B");
    }
}
