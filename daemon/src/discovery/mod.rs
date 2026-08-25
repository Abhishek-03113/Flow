//! Peer discovery for daemon-to-daemon pairing (`docs/architecture/channels.md`).

use flow_core::channel::ChannelAddress;
use flow_core::device::HostOs;

#[cfg(all(target_os = "linux", feature = "bluetooth"))]
pub mod bluetooth;
pub mod tcp;

/// A peer discovered over either medium — the shape both `discovery::tcp`
/// and `discovery::bluetooth` produce, so `G6`'s channel negotiation can
/// treat discoveries from either source uniformly.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredPeer {
    pub name: String,
    pub os: HostOs,
    pub address: ChannelAddress,
}
