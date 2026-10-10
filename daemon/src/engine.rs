//! The Flow engine as a library: storage, the service, the peer listener,
//! discovery, the per-peer input pipelines and (optionally) the loopback
//! control server, wired together from one [`EngineConfig`]. The
//! `flow-daemon` binary, the desktop app and the test harness all start
//! the engine through [`Engine::start`]; several engines can run isolated
//! in one process by giving each its own data dir and ports.

use std::collections::HashSet;
use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use flow_core::channel::Channel;
use flow_core::device::DeviceId;
use flow_core::input::InputInjector;
use flow_core::link::DaemonLinkState;
use flow_core::protocol::{InputEvent, InputRole};
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Mutex};

use crate::channel::tcp::TcpChannel;
use crate::devmode::SecurityMode;
use crate::discovery::tcp::DiscoveryService;
use crate::discovery::DiscoveredPeer;
use crate::ipc::{auth, server::handle_connection};
use crate::logging::LoggingHandle;
use crate::platform_backend::{BackendError, Injector, PlatformBackend};
use crate::security::Security;
use crate::service::{ConnectionPrecedence, DaemonService, IncomingPeerConnection, ServiceOptions};
use crate::storage::{history_logger, Storage};
use crate::{hotkey, pipeline};

/// Loopback WebSocket control server (`docs/contracts/daemon-ipc.md`).
#[derive(Debug, Clone)]
pub struct IpcConfig {
    /// `0` picks an ephemeral port; read it back from [`EngineHandle::ipc_addr`].
    pub port: u16,
    pub token_path: PathBuf,
}

/// UDP announce/listen used to find peers.
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    /// `0` picks an ephemeral port; read it back from [`EngineHandle::discovery_port`].
    pub port: u16,
    /// Where announces are sent. `None` broadcasts on every LAN interface;
    /// explicit addresses isolate engines sharing a host.
    pub targets: Option<Vec<SocketAddr>>,
}

pub struct EngineConfig {
    /// Holds `flow.db` (settings, devices, identity).
    pub data_dir: PathBuf,
    /// Overrides the hostname as this device's display name.
    pub device_name: Option<String>,
    /// `None` runs without a control server (the embedded app calls the
    /// service directly).
    pub ipc: Option<IpcConfig>,
    /// Peer-connection listener port; `0` = ephemeral.
    pub peer_listen_port: u16,
    /// `None` disables discovery (peers must be dialled explicitly).
    pub discovery: Option<DiscoveryConfig>,
    pub security: SecurityMode,
    pub backend: Arc<dyn PlatformBackend>,
    /// Enables the `debug_inject_input` command.
    pub test_hooks: bool,
    /// Seeds the mock-parity fixture instead of real state. Test fixture only.
    pub seed_mock_parity: bool,
    /// When set, `settings.debug_logging` drives its filter level.
    pub logging: Option<LoggingHandle>,
}

impl EngineConfig {
    /// A minimal config: no control server, no discovery, ephemeral peer
    /// port, secure mode.
    pub fn new(data_dir: impl Into<PathBuf>, backend: Arc<dyn PlatformBackend>) -> Self {
        Self {
            data_dir: data_dir.into(),
            device_name: None,
            ipc: None,
            peer_listen_port: 0,
            discovery: None,
            security: SecurityMode::Secure,
            backend,
            test_hooks: false,
            seed_mock_parity: false,
            logging: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("could not prepare the data directory {path}: {source}")]
    DataDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to open the flow-daemon database: {0}")]
    Storage(#[from] crate::storage::StorageError),
    #[error("failed to bind the IPC listener on 127.0.0.1:{port}: {source}")]
    IpcBind { port: u16, source: std::io::Error },
}

pub struct Engine;

impl Engine {
    pub async fn start(config: EngineConfig) -> Result<EngineHandle, EngineError> {
        std::fs::create_dir_all(&config.data_dir).map_err(|source| EngineError::DataDir {
            path: config.data_dir.clone(),
            source,
        })?;
        let storage = Storage::open(config.data_dir.join("flow.db")).await?;

        let service = if config.seed_mock_parity {
            tracing::warn!(
                "seeding the mock-parity test fixture instead of real state; \
                 never do this for a real deployment"
            );
            DaemonService::new_seeded_for_test(storage.clone()).await
        } else {
            let security = Security::from_mode(config.security);
            tracing::info!("peer-connection security profile: {}", security.label());
            DaemonService::new_with_options(
                storage.clone(),
                ServiceOptions {
                    security,
                    device_name: config.device_name.clone(),
                    host_os: config.backend.host_os(),
                },
            )
            .await
        };
        let service = Arc::new(service.with_test_hooks(config.test_hooks));
        if config.test_hooks {
            tracing::warn!("test hooks are enabled — the debug_inject_input command is available");
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let tasks = Tasks {
            shutdown: shutdown_rx,
        };
        let mut handle = EngineHandle {
            service: Arc::clone(&service),
            peer_port: None,
            ipc_addr: None,
            discovery_port: None,
            discovery_targets: Arc::new(std::sync::Mutex::new(Vec::new())),
            shutdown: shutdown_tx,
        };

        drop(history_logger::spawn(&service, storage.clone()));
        if let Some(logging) = config.logging.clone() {
            drop(crate::logging::spawn_debug_logging_toggle(
                &service, logging,
            ));
        }
        if let Some(runner) = hotkey::runner::spawn(&service, config.backend.as_ref()) {
            tasks.spawn(async move {
                let _ = runner.await;
            });
        }

        let runtime = Runtime {
            service: Arc::clone(&service),
            backend: Arc::clone(&config.backend),
            connected_peers: Arc::new(Mutex::new(HashSet::new())),
            tasks: tasks.clone(),
        };

        if let Some(port) = runtime.spawn_peer_listener(config.peer_listen_port).await {
            handle.peer_port = Some(port);
            if let Some(discovery) = &config.discovery {
                if let Some((bound, targets)) = runtime.spawn_discovery(discovery, port).await {
                    handle.discovery_port = Some(bound);
                    handle.discovery_targets = targets;
                }
            }
        }

        if let Some(ipc) = &config.ipc {
            handle.ipc_addr = Some(runtime.spawn_ipc(ipc).await?);
        }
        Ok(handle)
    }
}

/// What a started engine exposes. Dropping it without [`Self::shutdown`]
/// leaves the engine running.
pub struct EngineHandle {
    service: Arc<DaemonService>,
    peer_port: Option<u16>,
    ipc_addr: Option<SocketAddr>,
    discovery_port: Option<u16>,
    discovery_targets: Arc<std::sync::Mutex<Vec<SocketAddr>>>,
    shutdown: watch::Sender<bool>,
}

impl EngineHandle {
    /// The command/state surface the UI (or the IPC server) drives.
    pub fn service(&self) -> &Arc<DaemonService> {
        &self.service
    }

    /// The bound peer-listener port, if the listener started.
    pub fn peer_port(&self) -> Option<u16> {
        self.peer_port
    }

    /// The bound control-server address, if one was configured.
    pub fn ipc_addr(&self) -> Option<SocketAddr> {
        self.ipc_addr
    }

    /// The bound discovery port, if discovery started.
    pub fn discovery_port(&self) -> Option<u16> {
        self.discovery_port
    }

    /// Starts announcing to one more address; lets engines started on
    /// ephemeral ports find each other.
    pub fn add_discovery_target(&self, target: SocketAddr) {
        let mut targets = self
            .discovery_targets
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !targets.contains(&target) {
            targets.push(target);
        }
    }

    /// Stops every engine task and closes its sockets.
    pub fn shutdown(self) {
        let _ = self.shutdown.send(true);
    }
}

/// Spawns tasks that end when the engine shuts down.
#[derive(Clone)]
struct Tasks {
    shutdown: watch::Receiver<bool>,
}

impl Tasks {
    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) {
        let mut shutdown = self.shutdown.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = shutdown.wait_for(|stop| *stop) => {}
                _ = task => {}
            }
        });
    }
}

/// Devices this engine has a live, authenticated connection to — keyed by
/// the peer's proven identity, shared by the outbound (discovery redial)
/// and inbound (listener) paths so a peer is never connected twice.
type ConnectedPeers = Arc<Mutex<HashSet<DeviceId>>>;

#[derive(Clone)]
struct Runtime {
    service: Arc<DaemonService>,
    backend: Arc<dyn PlatformBackend>,
    connected_peers: ConnectedPeers,
    tasks: Tasks,
}

impl Runtime {
    async fn spawn_ipc(&self, config: &IpcConfig) -> Result<SocketAddr, EngineError> {
        // Every control connection must present this token as its WebSocket
        // subprotocol: loopback is reachable by any local process.
        let token: Arc<str> = Arc::from(auth::load_or_generate_token_at(&config.token_path));
        let listener = TcpListener::bind(("127.0.0.1", config.port))
            .await
            .map_err(|source| EngineError::IpcBind {
                port: config.port,
                source,
            })?;
        let addr = listener
            .local_addr()
            .expect("bound listener has a local address");
        tracing::info!(
            "IPC auth token: {}; listening on {addr}",
            config.token_path.display()
        );

        let service = Arc::clone(&self.service);
        let tasks = self.tasks.clone();
        self.tasks.spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        tracing::debug!("accepted connection from {peer}");
                        let service = Arc::clone(&service);
                        let token = Arc::clone(&token);
                        tasks.spawn(async move {
                            handle_connection(stream, service, token).await;
                        });
                    }
                    Err(err) => tracing::warn!("failed to accept connection: {err}"),
                }
            }
        });
        Ok(addr)
    }

    /// Binds the daemon-to-daemon listener on all interfaces (peers reach
    /// it over the LAN) and returns its port. A bind failure is logged and
    /// the engine keeps serving everything else.
    async fn spawn_peer_listener(&self, port: u16) -> Option<u16> {
        let listener = match TcpListener::bind(("0.0.0.0", port)).await {
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

        let runtime = self.clone();
        self.tasks.spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        tracing::debug!("accepted peer connection from {peer_addr}");
                        let inner = runtime.clone();
                        runtime.tasks.spawn(async move {
                            inner.handle_incoming_peer_stream(stream).await;
                        });
                    }
                    Err(err) => tracing::warn!("failed to accept peer connection: {err}"),
                }
            }
        });
        Some(port)
    }

    /// Starts discovery: one UDP socket both listens for announces and
    /// periodically announces this engine. Every parsed announce surfaces
    /// the peer as a pairing candidate and, if the peer is already paired,
    /// redials it so a returning device resumes without user action.
    async fn spawn_discovery(
        &self,
        config: &DiscoveryConfig,
        channel_port: u16,
    ) -> Option<(u16, Arc<std::sync::Mutex<Vec<SocketAddr>>>)> {
        let (name, os) = self.service.local_device_identity().await;
        let discovery = match DiscoveryService::bind(
            config.port,
            name,
            os,
            channel_port,
            self.service.local_instance_id(),
        )
        .await
        {
            Ok(discovery) => discovery,
            Err(err) => {
                tracing::warn!("discovery service not started: {err}");
                return None;
            }
        };
        let port = discovery.local_port().ok()?;
        tracing::info!("flow-daemon discovery listening on 0.0.0.0:{port}");

        let targets = Arc::new(std::sync::Mutex::new(
            config
                .targets
                .clone()
                .unwrap_or_else(|| DiscoveryService::broadcast_destinations(port)),
        ));
        let announce_targets = Arc::clone(&targets);
        let runtime = self.clone();
        self.tasks.spawn(async move {
            let mut announce_interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                tokio::select! {
                    _ = announce_interval.tick() => {
                        let destinations = announce_targets
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .clone();
                        for destination in destinations {
                            match discovery.announce_to(destination).await {
                                Ok(()) => crate::hop!(
                                    stage = "announce_sent",
                                    role = "local",
                                    to = %destination,
                                    "sent a discovery announce"
                                ),
                                Err(err) => crate::hop!(
                                    stage = "announce_failed",
                                    role = "local",
                                    to = %destination,
                                    error = %err,
                                    "a discovery announce send failed"
                                ),
                            }
                        }
                    }
                    received = discovery.recv_one() => match received {
                        Ok(Some(peer)) => runtime.handle_discovered_peer(peer).await,
                        Ok(None) => {}
                        // A transient receive error must not end discovery
                        // for the rest of the process's life.
                        Err(err) => tracing::warn!("discovery receive failed, continuing: {err}"),
                    }
                }
            }
        });
        Some((port, targets))
    }

    /// Everything past the cheap `note_discovered_peer` call runs on its
    /// own task: the dial does a TCP connect and a Noise handshake, and the
    /// pipeline it hands off to lives as long as the connection — awaiting
    /// either inside the discovery loop would stop this engine announcing.
    async fn handle_discovered_peer(&self, peer: DiscoveredPeer) {
        crate::hop!(
            stage = "discovered",
            role = "local",
            peer = %peer.name,
            address = ?peer.address,
            "discovery announce parsed for a peer"
        );
        self.service.note_discovered_peer(peer.clone()).await;

        let runtime = self.clone();
        self.tasks.spawn(async move {
            let Some((channel, device_id, precedence)) =
                runtime.service.dial_if_trusted(peer.address).await
            else {
                return;
            };
            runtime.claim_and_run(channel, device_id, precedence).await;
        });
    }

    /// One incoming connection: pairing attempts are handled in place,
    /// already-trusted peers get a pipeline.
    async fn handle_incoming_peer_stream(&self, stream: TcpStream) {
        let peer_addr = stream.peer_addr().ok();
        let channel: Box<dyn Channel> = match TcpChannel::accept(stream).await {
            Ok(channel) => Box::new(channel),
            Err(err) => {
                tracing::debug!("peer connection failed the WebSocket handshake: {err}");
                return;
            }
        };
        match self
            .service
            .accept_incoming_peer_channel(channel, peer_addr)
            .await
        {
            Ok(IncomingPeerConnection::HandledAsPairing) => {}
            Ok(IncomingPeerConnection::TrustedPeer(channel, device_id, precedence)) => {
                self.claim_and_run(channel, device_id, precedence).await;
            }
            Err(err) => tracing::debug!("incoming peer connection rejected: {err}"),
        }
    }

    /// Resolves this connection against any competing one to the same peer
    /// (both ends compute the same verdict, so exactly one survives) and
    /// runs the pipeline if it wins.
    async fn claim_and_run(
        &self,
        mut channel: Box<dyn Channel>,
        device_id: DeviceId,
        precedence: ConnectionPrecedence,
    ) {
        crate::hop_note!(
            stage = "claim",
            role = "local",
            peer = %device_id.0,
            precedence = ?precedence,
            "resolving this connection against any competing one to the same peer"
        );
        if precedence == ConnectionPrecedence::Redundant
            || !self.connected_peers.lock().await.insert(device_id.clone())
        {
            let _ = channel.close().await;
            crate::hop_note!(
                stage = "claim_dropped",
                role = "local",
                peer = %device_id.0,
                "dropped this connection as redundant"
            );
            return;
        }
        self.run_peer_pipeline(channel, device_id).await;
    }

    /// Runs the input-streaming pipeline over an authenticated connection.
    /// Releases the peer's claim however it ends, so a later reconnect
    /// isn't blocked.
    async fn run_peer_pipeline(&self, channel: Box<dyn Channel>, device_id: DeviceId) {
        let service = &self.service;
        crate::hop_note!(
            stage = "pipeline_up",
            role = "local",
            peer = %device_id.0,
            "input-streaming pipeline starting for a paired peer"
        );
        let devices_rx = service.watch_devices();
        let peer_name = devices_rx
            .borrow()
            .iter()
            .find(|device| device.id == device_id)
            .map(|device| device.name.clone())
            .unwrap_or_else(|| device_id.0.clone());

        let (capture_tx, capture_rx) = std::sync::mpsc::channel();
        let mut capture = match self.backend.start_capture(capture_tx) {
            Ok(capture) => capture,
            Err(err) => {
                crate::logging::product::error(
                    &format!(
                        "input capture would not start; pipeline for peer {peer_name} not running"
                    ),
                    &err,
                );
                self.connected_peers.lock().await.remove(&device_id);
                return;
            }
        };

        let (bridge_tx, bridge_rx) = tokio::sync::mpsc::unbounded_channel();
        let capture_bridge_tx = bridge_tx.clone();
        std::thread::spawn(move || {
            for event in capture_rx {
                if capture_bridge_tx.send(event).is_err() {
                    break;
                }
            }
        });

        // Synthetic events from `debug_inject_input` (test hooks) join the
        // very same stream, so they are gated, sequenced and injected
        // exactly like real ones.
        let debug_bridge_tx = bridge_tx.clone();
        let mut debug_rx = service.subscribe_debug_injections();
        tokio::spawn(async move {
            loop {
                match debug_rx.recv().await {
                    Ok(event) => {
                        if debug_bridge_tx.send(event).is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("debug injection lagged, dropped {n} synthetic events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        drop(bridge_tx);

        let injector = match self.backend.new_injector() {
            Ok(injector) => injector,
            Err(err) => {
                crate::logging::product::error(
                    &format!(
                        "input injector would not start; pipeline for peer {peer_name} not running"
                    ),
                    &err,
                );
                capture.stop();
                self.connected_peers.lock().await.remove(&device_id);
                return;
            }
        };

        let _switch_authority = service.enter_peer_pipeline();
        let pipeline_input =
            hotkey::runner::spawn_pipeline_switch_filter((**service).clone(), bridge_rx);

        let suppress_peer_name = peer_name.clone();
        let suppress_local = move |suppress: bool| {
            if let Err(err) = capture.set_suppress_local(suppress) {
                let action = if suppress { "suppress" } else { "restore" };
                crate::logging::product::error(
                    &format!(
                        "could not {action} local input for peer {suppress_peer_name} \
                         (input will also reach this machine's own applications)"
                    ),
                    &err,
                );
            }
        };

        let ownership_service = Arc::clone(service);
        let ownership_peer = device_id.clone();
        let on_peer_ownership = move |role: InputRole| {
            let service = Arc::clone(&ownership_service);
            let peer = ownership_peer.clone();
            tokio::spawn(async move { service.apply_peer_ownership(&peer, role).await });
        };

        service.set_link_state(DaemonLinkState::Connected);
        crate::logging::product::peer_connected(&peer_name);
        pipeline::run_paired_connection(
            channel,
            pipeline_input,
            devices_rx,
            BoxedInjector(injector),
            device_id.clone(),
            suppress_local,
            on_peer_ownership,
            service.ownership_handle(),
        )
        .await;
        crate::logging::product::peer_disconnected(&peer_name);
        crate::hop_note!(
            stage = "pipeline_down",
            role = "local",
            peer = %device_id.0,
            "input-streaming pipeline ended for this peer"
        );

        let no_peers_left = {
            let mut connected = self.connected_peers.lock().await;
            connected.remove(&device_id);
            connected.is_empty()
        };
        // Only downgrade link health once nothing else is still streaming.
        // `Reconnecting` while auto-reconnect is on, because discovery
        // keeps announcing and will redial on its own.
        if no_peers_left {
            let auto_reconnect = service.watch_settings().borrow().auto_reconnect;
            service.set_link_state(if auto_reconnect {
                DaemonLinkState::Reconnecting
            } else {
                DaemonLinkState::Disconnected
            });
        }
    }
}

/// Adapts a backend [`Injector`] to the pipeline's `InputInjector` bound.
struct BoxedInjector(Box<dyn Injector>);

impl InputInjector for BoxedInjector {
    type Error = BackendError;

    fn inject(&mut self, event: &InputEvent) -> Result<(), Self::Error> {
        self.0.inject(event)
    }
}
