//! Channel negotiation (`daemon/todos.json` G6): given the address(es) a
//! peer is reachable at, decide which concrete [`Channel`] implementation
//! to actually connect with. Per `docs/architecture/channels.md`'s
//! "Negotiation and fallback" section, this is the *only* place in the
//! daemon that knows both concrete `Channel` types exist — everything
//! upstream (G7 pairing, G8 streaming, H3 encryption) only ever holds a
//! `Box<dyn Channel>` after this point.
//!
//! Takes `&[ChannelAddress]` rather than a single `DiscoveredPeer`: each
//! discovery mechanism (`discovery::tcp`, `discovery::bluetooth`)
//! deliberately produces a `DiscoveredPeer` with exactly one
//! `ChannelAddress` — the medium *that* mechanism used — so a peer
//! "reachable via both mediums" can only be expressed once something
//! upstream has merged a TCP discovery and a Bluetooth discovery of the
//! same physical device into one address list (that merge — matching two
//! discoveries by identity — is track G7's job, alongside the pairing
//! candidate list it already builds). Reusing the existing
//! `ChannelAddress` enum for that list avoids inventing a parallel type
//! here.

use flow_core::channel::{Channel, ChannelAddress, ChannelError};

use super::tcp::TcpChannel;

/// Connects using the best available medium among `addresses`: TCP is
/// preferred whenever present — higher throughput and lower latency,
/// which matters for a continuous stream of mouse-move events — falling
/// back to Bluetooth only when no TCP address is known. Returns
/// [`ChannelError::Unreachable`] rather than hanging when `addresses` is
/// empty or contains only mediums this build can't use.
pub async fn connect_best_available(
    addresses: &[ChannelAddress],
) -> Result<Box<dyn Channel>, ChannelError> {
    if let Some(ChannelAddress::Tcp(addr)) = addresses
        .iter()
        .find(|addr| matches!(addr, ChannelAddress::Tcp(_)))
    {
        let channel = TcpChannel::connect(*addr).await?;
        return Ok(Box::new(channel));
    }
    if let Some(ChannelAddress::Bluetooth(addr)) = addresses
        .iter()
        .find(|addr| matches!(addr, ChannelAddress::Bluetooth(_)))
    {
        return connect_bluetooth(addr.clone()).await;
    }
    Err(ChannelError::Unreachable)
}

#[cfg(all(target_os = "linux", feature = "bluetooth"))]
async fn connect_bluetooth(
    addr: flow_core::channel::BluetoothAddr,
) -> Result<Box<dyn Channel>, ChannelError> {
    let channel = super::bluetooth::BluetoothChannel::connect(addr).await?;
    Ok(Box::new(channel))
}

/// Without the `bluetooth` feature (or off Linux), there's no
/// `BluetoothChannel` type to connect with at all — a Bluetooth-only
/// peer is unreachable in this build, reported the same way any other
/// unsupported medium would be, not silently treated as "no address
/// found."
#[cfg(not(all(target_os = "linux", feature = "bluetooth")))]
async fn connect_bluetooth(
    _addr: flow_core::channel::BluetoothAddr,
) -> Result<Box<dyn Channel>, ChannelError> {
    Err(ChannelError::UnsupportedMedium)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow_core::channel::{BluetoothAddr, ChannelKind};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn a_peer_reachable_via_both_mediums_resolves_to_tcp() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            let (stream, _peer) = listener.accept().await.expect("accept");
            let _ = TcpChannel::accept(stream).await;
        });

        let addresses = vec![
            ChannelAddress::Tcp(addr),
            ChannelAddress::Bluetooth(BluetoothAddr("AA:BB:CC:DD:EE:FF".to_string())),
        ];
        let channel = connect_best_available(&addresses)
            .await
            .expect("tcp candidate is reachable");
        assert_eq!(channel.kind(), ChannelKind::Tcp);
    }

    #[tokio::test]
    async fn a_peer_with_no_known_addresses_is_reported_unreachable_not_hanging() {
        let result = connect_best_available(&[]).await;
        assert_eq!(result.err(), Some(ChannelError::Unreachable));
    }

    /// Bluetooth-only, but this build has no `BluetoothChannel` to
    /// connect with (default build, no `bluetooth` feature, or a
    /// non-Linux target) — reported as `UnsupportedMedium` rather than
    /// silently falling through to `Unreachable`, so a caller can tell
    /// "no path existed" apart from "a path existed but this build can't
    /// use it."
    #[cfg(not(all(target_os = "linux", feature = "bluetooth")))]
    #[tokio::test]
    async fn a_bluetooth_only_peer_is_unsupported_when_this_build_has_no_bluetooth_channel() {
        let addresses = vec![ChannelAddress::Bluetooth(BluetoothAddr(
            "AA:BB:CC:DD:EE:FF".to_string(),
        ))];
        let result = connect_best_available(&addresses).await;
        assert_eq!(result.err(), Some(ChannelError::UnsupportedMedium));
    }

    /// The real Bluetooth-only path, requiring an actual `bluer`-backed
    /// connect attempt against a real peer. Ignored for the same reason
    /// as `channel::bluetooth`'s own loopback test: this container's
    /// kernel has no `AF_BLUETOOTH` support at all. Run manually on a
    /// Linux machine with a real Bluetooth adapter and `bluetoothd`
    /// running, alongside a listening peer:
    ///
    /// ```sh
    /// cargo test -p flow-daemon --features bluetooth --lib channel::negotiate -- --ignored
    /// ```
    #[cfg(all(target_os = "linux", feature = "bluetooth"))]
    #[ignore = "needs a real Bluetooth adapter + bluetoothd; this container's kernel has no AF_BLUETOOTH support at all"]
    #[tokio::test]
    async fn a_bluetooth_only_peer_resolves_to_bluetooth_on_a_real_adapter() {
        let addresses = vec![ChannelAddress::Bluetooth(BluetoothAddr(
            "AA:BB:CC:DD:EE:FF".to_string(),
        ))];
        let channel = connect_best_available(&addresses)
            .await
            .expect("a real, paired Bluetooth peer at this address");
        assert_eq!(channel.kind(), ChannelKind::Bluetooth);
    }
}
