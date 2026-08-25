//! Bluetooth peer discovery/advertisement (`daemon/todos.json` G5) — the
//! Bluetooth counterpart to `discovery::tcp`, producing the same
//! [`DiscoveredPeer`] shape so `G6`'s channel negotiation can treat both
//! discovery sources uniformly.
//!
//! Bluetooth Classic (BR/EDR, what [`BluetoothChannel`](crate::channel::bluetooth::BluetoothChannel)
//! uses) has no equivalent of a UDP broadcast payload — a nearby device is
//! found via BlueZ's inquiry scan, which only ever surfaces a device's
//! address and its self-reported *alias* (a short display name), not an
//! arbitrary byte payload. So instead of a dedicated wire packet like
//! `discovery::tcp::Announce`, this module encodes the same information
//! (peer name + OS) into the adapter's alias string, prefixed so it's
//! recognizable as a Flow daemon rather than any other nearby Bluetooth
//! device: `flow:{"name":"...","os":"..."}`.

use bluer::{Adapter, AdapterEvent, Address};
use flow_core::channel::{BluetoothAddr, ChannelAddress};
use flow_core::device::HostOs;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::DiscoveredPeer;

/// Every Flow-encoded alias starts with this, distinguishing it from an
/// arbitrary nearby Bluetooth device's own alias.
const ALIAS_PREFIX: &str = "flow:";

/// The payload encoded into a discoverable adapter's alias.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Advertisement {
    name: String,
    os: HostOs,
}

/// Encodes `name`/`os` into the alias string a Flow daemon advertises
/// itself under. Pure and unit-testable without a real adapter, unlike
/// everything else in this module.
fn encode_alias(name: &str, os: HostOs) -> String {
    let payload = Advertisement {
        name: name.to_string(),
        os,
    };
    format!(
        "{ALIAS_PREFIX}{}",
        serde_json::to_string(&payload).expect("serialize advertisement")
    )
}

/// Reverses [`encode_alias`]. Returns `None` for any alias that isn't a
/// Flow advertisement — expected for the common case of a nearby
/// non-Flow Bluetooth device turning up in a scan, not an error.
fn decode_alias(alias: &str) -> Option<Advertisement> {
    let json = alias.strip_prefix(ALIAS_PREFIX)?;
    serde_json::from_str(json).ok()
}

/// Advertises this daemon's presence: sets the adapter's alias to an
/// encoded `Advertisement` and makes it discoverable. Real deployment
/// calls this only while `start_pairing`'s `Searching` stage is active
/// (mirroring `discovery::tcp`'s equivalent note) — deciding *when* to
/// advertise is track G7's job, not this module's.
///
/// Requires a real Bluetooth adapter reachable via BlueZ's D-Bus API;
/// not exercised by this module's own tests (see the module-level
/// manual verification note in `daemon/README.md`).
pub async fn advertise(adapter: &Adapter, name: &str, os: HostOs) -> bluer::Result<()> {
    adapter.set_alias(encode_alias(name, os)).await?;
    adapter.set_discoverable(true).await
}

/// Spawns a background scan that reports every nearby device whose
/// alias decodes as a Flow advertisement, forwarding each as a
/// [`DiscoveredPeer`] on the returned channel. A non-Flow device
/// entering range is silently skipped, the same "not every packet on
/// this medium is ours" tolerance `discovery::tcp::recv_one` applies to
/// UDP traffic.
///
/// Requires a real Bluetooth adapter; not exercised by this module's
/// own tests.
pub async fn scan(adapter: &Adapter) -> bluer::Result<mpsc::UnboundedReceiver<DiscoveredPeer>> {
    let mut events = adapter.discover_devices().await?;
    let adapter = adapter.clone();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(event) = events.next().await {
            let AdapterEvent::DeviceAdded(address) = event else {
                continue;
            };
            if let Some(peer) = resolve_peer(&adapter, address).await {
                if tx.send(peer).is_err() {
                    break;
                }
            }
        }
    });
    Ok(rx)
}

/// Looks up a newly-seen device's alias and, if it decodes as a Flow
/// advertisement, builds the `DiscoveredPeer` for it. `None` covers both
/// "not a Flow device" and any transient BlueZ lookup failure — a scan
/// just keeps waiting for the next event either way.
async fn resolve_peer(adapter: &Adapter, address: Address) -> Option<DiscoveredPeer> {
    let device = adapter.device(address).ok()?;
    let alias = device.alias().await.ok()?;
    let advertisement = decode_alias(&alias)?;
    Some(DiscoveredPeer {
        name: advertisement.name,
        os: advertisement.os,
        address: ChannelAddress::Bluetooth(BluetoothAddr(address.to_string())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_advertised_alias_decodes_back_to_the_same_name_and_os() {
        let alias = encode_alias("Device A", HostOs::Linux);
        let decoded = decode_alias(&alias).expect("a flow advertisement");
        assert_eq!(decoded.name, "Device A");
        assert_eq!(decoded.os, HostOs::Linux);
    }

    #[test]
    fn the_encoded_alias_is_prefixed_so_its_recognizable_as_a_flow_device() {
        let alias = encode_alias("Device A", HostOs::Macos);
        assert!(alias.starts_with(ALIAS_PREFIX));
    }

    #[test]
    fn a_non_flow_alias_is_not_mistaken_for_an_advertisement() {
        assert_eq!(decode_alias("Some Random Headphones"), None);
    }

    #[test]
    fn a_flow_prefixed_but_malformed_payload_is_rejected_not_panicking() {
        assert_eq!(decode_alias("flow:not json"), None);
    }

    #[test]
    fn distinct_names_and_os_values_round_trip_independently() {
        for (name, os) in [
            ("Alice's Laptop", HostOs::Windows),
            ("bob-desktop", HostOs::Linux),
            ("", HostOs::Macos),
        ] {
            let alias = encode_alias(name, os);
            let decoded = decode_alias(&alias).expect("a flow advertisement");
            assert_eq!(decoded.name, name);
            assert_eq!(decoded.os, os);
        }
    }
}
