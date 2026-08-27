//! Auto-reconnect (`daemon/todos.json` I1): keeps a `Channel` to a peer
//! alive across drops, retrying with capped exponential backoff and
//! re-running `G6`'s negotiation (`connect_best_available`) on every
//! attempt — the medium that worked when first connected (e.g. Wi-Fi)
//! may not be the one available on recovery (e.g. Bluetooth-only),
//! which is exactly what re-negotiating each time is for
//! (`docs/architecture/channels.md`'s "Negotiation and fallback").
//! Drives a [`DaemonLinkState`] watch channel through the real
//! `daemon-ipc.md` transition table (`Connecting`/`Reconnecting` ->
//! `Connected`, or `Disconnected` once given up) as connection events
//! actually happen — matching `ServiceState::from_storage`'s own
//! `Disconnected` starting point, not a static `Connected` default.

use std::time::Duration;

use flow_core::channel::{Channel, ChannelAddress};
use flow_core::link::DaemonLinkState;
use tokio::sync::watch;

use super::negotiate::connect_best_available;

const INITIAL_BACKOFF: Duration = Duration::from_millis(100);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Establishes and maintains a connection to a peer reachable at any of
/// `addresses`, calling `handle_connection` with each freshly
/// (re)established `Channel`. `handle_connection` owns that channel
/// until it decides the connection is over (its own `recv`/`send`
/// reporting an error, typically — the same liveness signal
/// `pipeline::receive_and_inject` already uses) and returns; this
/// function then reconnects.
///
/// Loops forever *while* `settings.borrow().auto_reconnect` stays
/// `true`; the moment a connection attempt is about to retry and finds
/// it `false`, this returns instead, leaving `link_state` at
/// `Disconnected` — "unreachable and not retrying," per
/// `DaemonLinkState`'s own doc comment.
pub async fn maintain_connection<F, Fut>(
    addresses: Vec<ChannelAddress>,
    settings: watch::Receiver<flow_core::settings::FlowSettings>,
    link_state: watch::Sender<DaemonLinkState>,
    mut handle_connection: F,
) where
    F: FnMut(Box<dyn Channel>) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut has_connected_before = false;
    let mut backoff = INITIAL_BACKOFF;

    loop {
        link_state.send_replace(if has_connected_before {
            DaemonLinkState::Reconnecting
        } else {
            DaemonLinkState::Connecting
        });

        if let Ok(channel) = connect_best_available(&addresses).await {
            link_state.send_replace(DaemonLinkState::Connected);
            has_connected_before = true;
            backoff = INITIAL_BACKOFF;
            handle_connection(channel).await;
            // `handle_connection` returned: the connection ended. Fall
            // through to retry, subject to the same auto_reconnect
            // check a failed connect attempt uses.
        }

        if !settings.borrow().auto_reconnect {
            link_state.send_replace(DaemonLinkState::Disconnected);
            return;
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::tcp::TcpChannel;
    use flow_core::settings::FlowSettings;
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::time::timeout;

    /// A peer that accepts a first connection and immediately closes it
    /// (simulating the link dropping right after connecting), then
    /// accepts a second connection and holds it open by blocking on
    /// `recv()` — the reconnect target. Both connections come from the
    /// same bound listener/address, so `maintain_connection` reaches
    /// the second one by simply retrying the same `addresses`.
    async fn spawn_peer_that_drops_once_then_stays_up() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind peer listener");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            let (stream, _peer) = listener.accept().await.expect("accept first");
            let mut first = TcpChannel::accept(stream).await.expect("accept ws first");
            let _ = first.close().await;

            let (stream, _peer) = listener.accept().await.expect("accept second");
            let mut second = TcpChannel::accept(stream).await.expect("accept ws second");
            let _ = second.recv().await;
        });
        addr
    }

    async fn wait_for_link_state(
        rx: &mut watch::Receiver<DaemonLinkState>,
        target: DaemonLinkState,
    ) {
        timeout(Duration::from_secs(2), async {
            loop {
                if *rx.borrow() == target {
                    return;
                }
                rx.changed().await.expect("link state channel stays open");
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {target:?}"));
    }

    #[tokio::test]
    async fn a_dropped_connection_reconnects_and_recovers_link_state() {
        let addr = spawn_peer_that_drops_once_then_stays_up().await;
        let addresses = vec![ChannelAddress::Tcp(addr)];
        let (_settings_tx, settings_rx) = watch::channel(FlowSettings::defaults());
        let (link_state_tx, mut link_state_rx) = watch::channel(DaemonLinkState::Disconnected);
        let _ = link_state_rx.borrow_and_update();

        let connection_count = Arc::new(AtomicUsize::new(0));
        let connection_count_for_task = connection_count.clone();

        let maintainer = tokio::spawn(async move {
            maintain_connection(addresses, settings_rx, link_state_tx, move |mut channel| {
                let connection_count = connection_count_for_task.clone();
                async move {
                    connection_count.fetch_add(1, Ordering::SeqCst);
                    // Blocks until this connection ends: the peer's own
                    // `close()` (first connection) or the test itself
                    // aborting the maintainer task (second connection).
                    let _ = channel.recv().await;
                }
            })
            .await;
        });

        wait_for_link_state(&mut link_state_rx, DaemonLinkState::Connected).await;
        wait_for_link_state(&mut link_state_rx, DaemonLinkState::Reconnecting).await;
        wait_for_link_state(&mut link_state_rx, DaemonLinkState::Connected).await;

        assert_eq!(connection_count.load(Ordering::SeqCst), 2);
        maintainer.abort();
    }

    #[tokio::test]
    async fn giving_up_when_auto_reconnect_is_disabled_lands_on_disconnected() {
        // Bind-then-drop reserves a real port nothing is listening on,
        // so every connect attempt fails immediately with a genuine
        // connection-refused rather than an arbitrary invalid address.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        drop(listener);

        let mut settings = FlowSettings::defaults();
        settings.auto_reconnect = false;
        let (_settings_tx, settings_rx) = watch::channel(settings);
        let (link_state_tx, mut link_state_rx) = watch::channel(DaemonLinkState::Connected);
        let _ = link_state_rx.borrow_and_update();

        let addresses = vec![ChannelAddress::Tcp(addr)];
        maintain_connection(
            addresses,
            settings_rx,
            link_state_tx,
            |_channel: Box<dyn Channel>| async {
                unreachable!("nothing is listening; a connection should never succeed")
            },
        )
        .await;

        assert_eq!(*link_state_rx.borrow(), DaemonLinkState::Disconnected);
    }
}
