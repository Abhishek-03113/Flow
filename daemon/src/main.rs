//! Flow daemon entry point.
//!
//! Per vision.md §8, the daemon must work with no UI attached: it opens
//! its SQLite-backed state, starts the connection-history logger, and
//! serves the local IPC contract (`docs/contracts/daemon-ipc.md`) over a
//! WebSocket on `127.0.0.1:IPC_PORT` until the process is asked to stop.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use flow_core::channel::Channel;
use flow_core::device::DeviceId;
use flow_core::input::{InputCapture, InputInjector};
use flow_core::ipc::IPC_PORT;
use flow_core::link::DaemonLinkState;
use flow_core::protocol::InputEvent;
use flow_daemon::channel::tcp::TcpChannel;
use flow_daemon::discovery::tcp::{DiscoveryService, DISCOVERY_PORT};
use flow_daemon::discovery::DiscoveredPeer;
use flow_daemon::hotkey;
use flow_daemon::ipc::auth;
use flow_daemon::ipc::server::handle_connection;
use flow_daemon::pipeline;
use flow_daemon::service::{ConnectionPrecedence, DaemonService, IncomingPeerConnection};
use flow_daemon::storage::{history_logger, Storage};
use flow_platform::{new_default_input_injector, DefaultInputCapture};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// Devices this daemon currently has a live, authenticated
/// daemon-to-daemon connection to — keyed by the peer's proven identity
/// (`DeviceId`, derived from its `H1` public key), not its address or
/// name, both of which can change between reconnects. Shared between the
/// outbound (discovery-driven redial) and inbound (peer channel accept
/// loop) paths purely to avoid opening a second, redundant connection to
/// a peer this daemon is already streaming input with.
type ConnectedPeers = Arc<Mutex<HashSet<DeviceId>>>;

#[tokio::main]
async fn main() {
    // Opt-in dev/testing switches, resolved once up front (all inert
    // unless their env vars are set) — see `flow_daemon::devmode`.
    let dev = flow_daemon::devmode::DevMode::from_env();

    // `debug_logging`'s persisted value isn't known until settings load
    // below; starts at the non-debug level and gets synced once it is.
    // `FLOW_TRACE` pins the verbose `TRACE` floor on for the whole run.
    let logging = flow_daemon::logging::init(false, dev.trace);
    for warning in &dev.warnings {
        tracing::warn!("{warning}");
    }
    if let Some(banner) = dev.insecure_banner() {
        tracing::warn!("\n{banner}");
    }

    let storage = match Storage::open(db_path()).await {
        Ok(storage) => storage,
        Err(err) => fatal(&format!("failed to open the flow-daemon database: {err}")),
    };
    let service = Arc::new(daemon_service(storage.clone()).await);
    let _history_logger = history_logger::spawn(&service, storage.clone());
    let _hotkey_runner = hotkey::runner::spawn(&service);
    let _debug_logging_toggle = flow_daemon::logging::spawn_debug_logging_toggle(&service, logging);

    let connected_peers: ConnectedPeers = Arc::new(Mutex::new(HashSet::new()));
    if let Some(peer_channel_port) =
        spawn_peer_channel_listener(Arc::clone(&service), Arc::clone(&connected_peers)).await
    {
        spawn_discovery(Arc::clone(&service), connected_peers, peer_channel_port).await;
    }

    // Every IPC connection must present this token (`auth::token_path()`)
    // as its WebSocket subprotocol — `127.0.0.1` is reachable by any
    // local process, not just the intended Flutter UI, and this is what
    // actually tells the two apart now instead of trusting the loopback
    // address alone.
    let ipc_token: Arc<str> = Arc::from(auth::load_or_generate_token());
    tracing::info!("IPC auth token: {}", auth::token_path().display());

    let ipc_port = ipc_port();
    let listener = match TcpListener::bind(("127.0.0.1", ipc_port)).await {
        Ok(listener) => listener,
        Err(err) => fatal(&format!(
            "failed to bind the IPC listener on 127.0.0.1:{ipc_port}: {err}\n\
             (another flow-daemon may already be running on this port — set \
             FLOW_IPC_PORT to run a second instance)"
        )),
    };
    tracing::info!("flow-daemon listening on 127.0.0.1:{ipc_port}");

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, peer)) => {
                        tracing::debug!("accepted connection from {peer}");
                        let service = Arc::clone(&service);
                        let ipc_token = Arc::clone(&ipc_token);
                        tokio::spawn(async move {
                            handle_connection(stream, service, ipc_token).await;
                        });
                    }
                    Err(e) => {
                        tracing::warn!("failed to accept connection: {e}");
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("shutting down");
                break;
            }
        }
    }
}

/// Binds the daemon-to-daemon peer channel listener — distinct from the
/// local IPC port above (Flutter<->daemon) and from the discovery UDP
/// port (`DISCOVERY_PORT`, announce/listen only) — and spawns its accept
/// loop. Bound to `0.0.0.0` (unlike the IPC listener's `127.0.0.1`-only
/// binding) since real peers reach it from elsewhere on the LAN. An
/// ephemeral port (`0`) is used and the actual bound port is returned so
/// [`spawn_discovery`] can advertise it in this daemon's own announces
/// (`discovery::tcp`'s own doc comment: "the port this peer's own
/// `TcpChannel` listener is bound to"). Returns `None` — logging a
/// warning, not panicking — if the bind itself fails; the daemon still
/// serves local IPC and the switch-key hotkey normally without it, the
/// same "degrade gracefully" contract `hotkey::runner::spawn` already
/// has for a missing capture device.
async fn spawn_peer_channel_listener(
    service: Arc<DaemonService>,
    connected_peers: ConnectedPeers,
) -> Option<u16> {
    let listener = match TcpListener::bind(("0.0.0.0", 0)).await {
        Ok(listener) => listener,
        Err(err) => {
            tracing::warn!("peer channel listener not started: {err}");
            return None;
        }
    };
    let port = listener
        .local_addr()
        .expect("bound listener has a local address")
        .port();
    tracing::info!("flow-daemon peer channel listening on 0.0.0.0:{port}");

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    tracing::debug!("accepted peer connection from {peer_addr}");
                    let service = Arc::clone(&service);
                    let connected_peers = Arc::clone(&connected_peers);
                    tokio::spawn(async move {
                        handle_incoming_peer_stream(service, stream, connected_peers).await;
                    });
                }
                Err(err) => {
                    tracing::warn!("failed to accept peer connection: {err}");
                }
            }
        }
    });

    Some(port)
}

/// Starts this daemon's TCP peer discovery (`discovery::tcp`, track G3):
/// a single UDP socket both listens for other daemons' announces and
/// periodically broadcasts this one's own presence, feeding every
/// successfully-parsed announce to two independent consumers —
/// `DaemonService::note_discovered_peer` (unchanged existing behavior:
/// surfaces the peer as a pairing candidate) and a redial attempt for
/// the case where the announcing peer turns out to already be a trusted,
/// paired device (`DaemonService::dial_if_trusted`) — so a previously
/// paired device that becomes reachable again (Wi-Fi reconnect, the
/// other daemon restarting) resumes streaming without any user action.
async fn spawn_discovery(
    service: Arc<DaemonService>,
    connected_peers: ConnectedPeers,
    channel_port: u16,
) {
    let (name, os) = service.local_device_identity().await;
    // Our own identity key doubles as the announce instance id: unique
    // per daemon by construction, and already known here, so a broadcast
    // echoing back to this host is recognizable as ours.
    let instance_id = service.local_instance_id();

    let discovery =
        match DiscoveryService::bind(DISCOVERY_PORT, name, os, channel_port, instance_id).await {
            Ok(discovery) => discovery,
            Err(err) => {
                tracing::warn!("discovery service not started: {err}");
                return;
            }
        };
    tracing::info!("flow-daemon discovery listening on 0.0.0.0:{DISCOVERY_PORT}");
    let destinations = DiscoveryService::broadcast_destinations();
    tracing::info!("flow-daemon announcing to {destinations:?} every 5s");

    tokio::spawn(async move {
        let mut announce_interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            tokio::select! {
                _ = announce_interval.tick() => {
                    for destination in &destinations {
                        match discovery.announce_to(*destination).await {
                            Ok(()) => flow_daemon::hop!(
                                stage = "announce_sent",
                                role = "local",
                                to = %destination,
                                "sent a discovery announce"
                            ),
                            Err(err) => flow_daemon::hop!(
                                stage = "announce_failed",
                                role = "local",
                                to = %destination,
                                error = %err,
                                "a discovery announce send failed"
                            ),
                        }
                    }
                }
                received = discovery.recv_one() => {
                    match received {
                        Ok(Some(peer)) => {
                            handle_discovered_peer(&service, peer, &connected_peers).await;
                        }
                        Ok(None) => continue,
                        // A transient receive error (an ICMP
                        // unreachable landing on the socket, an
                        // interface going down) must not end discovery
                        // for the rest of the process's life — the next
                        // tick just tries again.
                        Err(err) => {
                            tracing::warn!("discovery receive failed, continuing: {err}");
                        }
                    }
                }
            }
        }
    });
}

/// One discovered peer, fed to both consumers described in
/// [`spawn_discovery`]'s doc comment.
///
/// Everything past the (cheap, local) `note_discovered_peer` call runs on
/// its own task rather than inline. The dial does a TCP connect and a
/// Noise handshake, and the pipeline it hands off to runs for the whole
/// lifetime of the connection — awaiting any of that inside the
/// discovery `select!` loop would stop this daemon announcing itself and
/// reading others' announces for as long as a single peer stayed
/// connected, so no third device could ever discover it.
async fn handle_discovered_peer(
    service: &Arc<DaemonService>,
    peer: DiscoveredPeer,
    connected_peers: &ConnectedPeers,
) {
    flow_daemon::hop!(
        stage = "discovered",
        role = "local",
        peer = %peer.name,
        address = ?peer.address,
        "discovery announce parsed for a peer"
    );
    service.note_discovered_peer(peer.clone()).await;

    let service = Arc::clone(service);
    let connected_peers = Arc::clone(connected_peers);
    tokio::spawn(async move {
        let Some((channel, device_id, precedence)) = service.dial_if_trusted(peer.address).await
        else {
            return;
        };
        claim_and_run(service, channel, device_id, precedence, connected_peers).await;
    });
}

/// Shared tail of both connection paths (outbound dial and inbound
/// accept): resolve the connection against any competing one to the same
/// peer, and run the pipeline if this one wins.
async fn claim_and_run(
    service: Arc<DaemonService>,
    channel: Box<dyn Channel>,
    device_id: DeviceId,
    precedence: ConnectionPrecedence,
    connected_peers: ConnectedPeers,
) {
    flow_daemon::hop_note!(
        stage = "claim",
        role = "local",
        peer = %device_id.0,
        precedence = ?precedence,
        "resolving this connection against any competing one to the same peer"
    );
    // A `Redundant` verdict means the peer's competing connection is the
    // designated keeper, so drop this one without even trying to claim.
    // Both ends compute this identically, so exactly one of the two
    // survives — see `DaemonService::connection_precedence`.
    if precedence == ConnectionPrecedence::Redundant {
        let mut channel = channel;
        let _ = channel.close().await;
        flow_daemon::hop_note!(
            stage = "claim_dropped",
            role = "local",
            peer = %device_id.0,
            "dropped this connection as redundant"
        );
        return;
    }
    if !try_claim_peer(&connected_peers, &device_id).await {
        let mut channel = channel;
        let _ = channel.close().await;
        flow_daemon::hop_note!(
            stage = "claim_lost",
            role = "local",
            peer = %device_id.0,
            "another task already holds this peer's connection slot"
        );
        return;
    }
    run_peer_pipeline(service, channel, device_id, connected_peers).await;
}

/// One incoming daemon-to-daemon connection on the peer channel
/// listener: dispatches via `DaemonService::accept_incoming_peer_channel`
/// and, for an already-trusted peer, runs the streaming pipeline —
/// deduplicated against `connected_peers` the same way the outbound
/// (discovery-driven) path is, since both an inbound and an outbound
/// connection attempt to the same peer can race in a real deployment.
async fn handle_incoming_peer_stream(
    service: Arc<DaemonService>,
    stream: TcpStream,
    connected_peers: ConnectedPeers,
) {
    let peer_addr = stream.peer_addr().ok();
    let channel: Box<dyn Channel> = match TcpChannel::accept(stream).await {
        Ok(channel) => Box::new(channel),
        Err(err) => {
            tracing::debug!("peer connection failed the WebSocket handshake: {err}");
            return;
        }
    };
    match service
        .accept_incoming_peer_channel(channel, peer_addr)
        .await
    {
        Ok(IncomingPeerConnection::HandledAsPairing) => {}
        Ok(IncomingPeerConnection::TrustedPeer(channel, device_id, precedence)) => {
            claim_and_run(service, channel, device_id, precedence, connected_peers).await;
        }
        Err(err) => {
            tracing::debug!("incoming peer connection rejected: {err}");
        }
    }
}

/// Atomically checks-and-inserts `device_id` into `connected_peers`,
/// returning whether this caller "won" and should proceed to run the
/// pipeline. Guards against both directions (an outbound redial racing
/// an inbound accept for the same paired peer) trying to run two
/// concurrent connections to one device at once.
async fn try_claim_peer(connected_peers: &ConnectedPeers, device_id: &DeviceId) -> bool {
    connected_peers.lock().await.insert(device_id.clone())
}

/// Runs the real input-streaming pipeline (`pipeline::run_paired_connection`)
/// over an already-authenticated connection to `device_id`, using a
/// dedicated real per-OS capture/injector pair for the duration of this
/// one connection — the same construction `hotkey::runner::spawn` already
/// uses for its own, separate capture instance, since OS input capture
/// supports more than one independent listener. Degrades gracefully, not
/// fatally, when the platform adapter can't start (no capturable device,
/// missing permission), matching the hotkey runner's own behavior: this
/// one peer connection's pipeline doesn't start, but the rest of the
/// daemon (IPC, other peer connections) is unaffected. Always releases
/// `device_id`'s claim in `connected_peers` on the way out, however this
/// ends, so a later reconnect isn't wrongly blocked forever.
async fn run_peer_pipeline(
    service: Arc<DaemonService>,
    channel: Box<dyn Channel>,
    device_id: DeviceId,
    connected_peers: ConnectedPeers,
) {
    tracing::info!("streaming pipeline starting for peer {device_id:?}");
    flow_daemon::hop_note!(
        stage = "pipeline_up",
        role = "local",
        peer = %device_id.0,
        "input-streaming pipeline starting for a paired peer"
    );
    let devices_rx = service.watch_devices();

    let (capture_tx, capture_rx) = std::sync::mpsc::channel();
    let mut capture = DefaultInputCapture::new(capture_tx);
    if let Err(err) = capture.start() {
        tracing::warn!(
            "peer pipeline for {device_id:?} not started: input capture failed: {err:?}"
        );
        flow_daemon::hop_note!(
            stage = "capture_failed",
            role = "owner",
            peer = %device_id.0,
            "input capture would not start; this peer's pipeline is not running"
        );
        connected_peers.lock().await.remove(&device_id);
        return;
    }
    flow_daemon::hop_note!(
        stage = "capture_started",
        role = "owner",
        peer = %device_id.0,
        "OS input capture started for this connection"
    );

    let (bridge_tx, bridge_rx) = tokio::sync::mpsc::unbounded_channel();
    let bridge_device_id = device_id.clone();
    std::thread::spawn(move || {
        for event in capture_rx {
            flow_daemon::hop!(
                stage = "captured",
                role = "owner",
                peer = %bridge_device_id.0,
                kind = pipeline::event_kind(&event),
                "raw event captured from the OS"
            );
            if bridge_tx.send(event).is_err() {
                break;
            }
        }
    });

    // The real injector (e.g. `MacosInputInjector`, wrapping a
    // `CGEventSource`) can be platform-thread-affine — its Core
    // Foundation pointer type isn't `Send` — and this whole function
    // runs inside `tokio::spawn`, which requires every value held across
    // an `.await` to be `Send`. `spawn_injector` keeps the real injector
    // confined to its own dedicated OS thread and hands back a `Send`,
    // channel-backed [`InjectorHandle`] instead, so the non-`Send` type
    // never enters this async function's state machine at all.
    let Some(injector) = spawn_injector(device_id.clone()) else {
        let _ = capture.stop();
        connected_peers.lock().await.remove(&device_id);
        return;
    };

    // Suppression runs on the capture handle, which the pipeline can't
    // hold itself (it lives here, alongside the thread bridging capture
    // into async). A failure is logged once per transition rather than
    // per event, and never aborts the connection: on macOS and Windows
    // this always fails today (see `InputCapture::set_suppress_local`),
    // and streaming input to the peer is still useful there even while
    // it also reaches local applications.
    let suppression_device_id = device_id.clone();
    let suppress_local = move |suppress: bool| {
        if let Err(err) = capture.set_suppress_local(suppress) {
            tracing::warn!(
                "could not {} local input for peer {suppression_device_id:?}: {err:?} \
                 (input will reach this machine's own applications as well)",
                if suppress { "suppress" } else { "restore" }
            );
        }
    };

    service.set_link_state(DaemonLinkState::Connected);
    flow_daemon::hop_note!(
        stage = "link_connected",
        role = "local",
        peer = %device_id.0,
        "link state set to Connected; streaming both directions now"
    );
    pipeline::run_paired_connection(
        channel,
        bridge_rx,
        devices_rx,
        injector,
        device_id.clone(),
        suppress_local,
    )
    .await;
    tracing::info!("streaming pipeline ended for peer {device_id:?}");
    flow_daemon::hop_note!(
        stage = "pipeline_down",
        role = "local",
        peer = %device_id.0,
        "input-streaming pipeline ended for this peer"
    );

    let no_peers_left = {
        let mut connected = connected_peers.lock().await;
        connected.remove(&device_id);
        connected.is_empty()
    };
    // Only downgrade link health once nothing else is still streaming —
    // another paired device's connection may well still be live.
    // `Reconnecting` rather than `Disconnected` because discovery keeps
    // listening and re-announcing in the background and will redial this
    // peer, or any other, with no user action — which is exactly what
    // `daemon-ipc.md`'s "connected -> (active link drops) -> reconnecting"
    // transition describes. `Disconnected` ("unreachable and not
    // retrying") is reserved for auto_reconnect being switched off, the
    // one case where nothing will retry on its own.
    if no_peers_left {
        let auto_reconnect = service.watch_settings().borrow().auto_reconnect;
        service.set_link_state(if auto_reconnect {
            DaemonLinkState::Reconnecting
        } else {
            DaemonLinkState::Disconnected
        });
    }
}

/// A `Send`, channel-backed [`InputInjector`] that stands in for the
/// platform's real injector inside `run_peer_pipeline`'s `tokio::spawn`ed
/// task. The real injector can be platform-thread-affine (on macOS,
/// `MacosInputInjector` wraps a `CGEventSource`, whose Core Foundation
/// pointer type isn't `Send`), so it's never held here — `inject` just
/// forwards the event down a channel to the dedicated OS thread
/// [`spawn_injector`] started, which owns the real injector and is the
/// only thing that ever touches it. The same OS-thread bridging already
/// used for `capture_rx`/`bridge_tx` in `run_peer_pipeline`, just in the
/// injection direction.
struct InjectorHandle {
    events: std::sync::mpsc::Sender<InputEvent>,
}

impl InputInjector for InjectorHandle {
    type Error = std::sync::mpsc::SendError<InputEvent>;

    fn inject(&mut self, event: &InputEvent) -> Result<(), Self::Error> {
        self.events.send(event.clone())
    }
}

/// Starts the platform's real input injector (`new_default_input_injector`)
/// on a dedicated OS thread — so its possibly non-`Send` type never has to
/// cross an `.await` — and returns a [`InjectorHandle`] to it. `None`
/// (logging a warning, not panicking) if the platform adapter can't
/// start, the same "degrade gracefully" contract every other
/// platform-adapter construction in this file has. Blocks briefly on
/// `ready_rx.recv()` to learn whether construction succeeded before
/// returning, mirroring `MacosInputCapture::start()`'s own
/// ready-channel handshake for its capture thread.
fn spawn_injector(device_id: DeviceId) -> Option<InjectorHandle> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (events_tx, events_rx) = std::sync::mpsc::channel::<InputEvent>();

    std::thread::spawn(move || {
        let mut injector = match new_default_input_injector() {
            Ok(injector) => injector,
            Err(err) => {
                tracing::warn!(
                    "peer pipeline for {device_id:?} not started: input injection failed: {err:?}"
                );
                let _ = ready_tx.send(false);
                return;
            }
        };
        if ready_tx.send(true).is_err() {
            // `spawn_injector`'s caller gave up waiting — nothing left
            // to inject for.
            return;
        }
        for event in events_rx {
            if let Err(err) = injector.inject(&event) {
                tracing::warn!("input injection failed for peer {device_id:?}: {err:?}");
            }
        }
    });

    match ready_rx.recv() {
        Ok(true) => Some(InjectorHandle { events: events_tx }),
        _ => None,
    }
}

/// Real production state (`DaemonService::new`/`ServiceState::from_storage`)
/// unless `FLOW_DAEMON_SEED_MOCK_PARITY` is set in the environment, in
/// which case this seeds the exact mock-parity fixture
/// (`DaemonService::new_seeded_for_test`/`ServiceState::seeded_for_test`)
/// instead. This exists purely so
/// `flutter/test/data/ipc_daemon_repository_manual_test.dart` — a
/// cross-language contract test that asserts on the mock's specific
/// seeded devices/candidates — has a real `flow-daemon` process to run
/// against without a real daemon ever seeding fake data by default. A
/// real deployment must never set this; nothing in this codebase sets it
/// automatically.
async fn daemon_service(storage: Storage) -> DaemonService {
    if std::env::var_os("FLOW_DAEMON_SEED_MOCK_PARITY").is_some() {
        tracing::warn!(
            "FLOW_DAEMON_SEED_MOCK_PARITY is set — seeding the mock-parity \
             test fixture instead of real state. Never set this for a real \
             deployment."
        );
        DaemonService::new_seeded_for_test(storage).await
    } else {
        DaemonService::new(storage).await
    }
}

/// The database file lives under the platform data directory (via the
/// `directories` crate) rather than the working directory, so
/// `flow-daemon` behaves the same regardless of where it's launched from.
///
/// `FLOW_DATA_DIR` overrides that directory outright — the supported way
/// to run a second `flow-daemon` instance on one machine (its own
/// database and its own persisted ed25519 identity, distinct from the
/// default instance's) for local two-daemon testing. Unset in a normal
/// deployment.
fn db_path() -> PathBuf {
    let dir = match std::env::var_os("FLOW_DATA_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => directories::ProjectDirs::from("dev", "Flow", "flow-daemon")
            .map(|dirs| dirs.data_dir().to_path_buf())
            .unwrap_or_else(|| fatal("could not determine the platform data directory")),
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        fatal(&format!(
            "failed to create the data directory {}: {err}",
            dir.display()
        ));
    }
    dir.join("flow.db")
}

/// The IPC listener port — [`IPC_PORT`] unless `FLOW_IPC_PORT` overrides
/// it (paired with `FLOW_DATA_DIR` and `FLOW_IPC_TOKEN_PATH` to run a
/// second local instance). An unparseable value is a hard error rather
/// than a silent fall back to the default, since that would land the
/// second instance on the first instance's port.
fn ipc_port() -> u16 {
    match std::env::var("FLOW_IPC_PORT") {
        Ok(raw) => raw.parse().unwrap_or_else(|_| {
            fatal(&format!(
                "FLOW_IPC_PORT is not a valid port number: {raw:?}"
            ))
        }),
        Err(std::env::VarError::NotPresent) => IPC_PORT,
        Err(std::env::VarError::NotUnicode(_)) => fatal("FLOW_IPC_PORT is not valid UTF-8"),
    }
}

/// Prints a one-line reason to stderr and exits non-zero. Used for
/// unrecoverable startup misconfiguration (a port already in use, an
/// unwritable data directory) so `flow-daemon` fails with a readable
/// message instead of a panic backtrace.
fn fatal(message: &str) -> ! {
    eprintln!("flow-daemon: {message}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three env overrides that let a second `flow-daemon` instance
    /// run on one machine are read in exactly one place each; this pins
    /// down `ipc_port()`'s parsing (the others just substitute a path).
    #[test]
    fn ipc_port_defaults_and_overrides() {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os("FLOW_IPC_PORT");

        std::env::remove_var("FLOW_IPC_PORT");
        assert_eq!(
            ipc_port(),
            IPC_PORT,
            "unset falls back to the well-known port"
        );

        std::env::set_var("FLOW_IPC_PORT", "47999");
        assert_eq!(ipc_port(), 47999, "a valid value overrides the default");

        match previous {
            Some(value) => std::env::set_var("FLOW_IPC_PORT", value),
            None => std::env::remove_var("FLOW_IPC_PORT"),
        }
    }
}
