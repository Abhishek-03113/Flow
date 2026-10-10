//! The Flow engine as a library: storage, the service, the peer listener,
//! discovery, the per-peer input pipelines and (optionally) the loopback
//! control server, wired together from one [`EngineConfig`]. The
//! `flow-daemon` binary, the desktop app and the test harness all start
//! the engine through [`Engine::start`]; several engines can run isolated
//! in one process by giving each its own data dir and ports.

use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use flow_core::channel::Channel;
use flow_core::control::Role;
use flow_core::device::DeviceId;
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use crate::channel::tcp::TcpChannel;
use crate::devmode::SecurityMode;
use crate::discovery::tcp::DiscoveryService;
use crate::discovery::DiscoveredPeer;
use crate::ipc::{auth, server::handle_connection};
use crate::link::{run_link, LinkEnv, LinkTiming};
use crate::logging::LoggingHandle;
use crate::platform_backend::{CaptureGuard, PlatformBackend};
use crate::security::Security;
use crate::service::{ConnectionPrecedence, DaemonService, IncomingPeerConnection, ServiceOptions};
use crate::storage::{history_logger, Storage};

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
    /// How often this engine announces itself.
    pub announce_interval: Duration,
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
    /// Where the user's keymap and remap files live; `None` = built-in
    /// defaults only.
    pub keymap_dir: Option<PathBuf>,
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
            keymap_dir: None,
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
                    screen: config.backend.screen_size(),
                    keymap_dir: config.keymap_dir.clone(),
                    backend: Some(Arc::clone(&config.backend)),
                },
            )
            .await
        };
        let service = Arc::new(
            service
                .with_test_hooks(config.test_hooks)
                .with_diagnostics_export(
                    Some(config.data_dir.join("diagnostics")),
                    config.logging.clone(),
                ),
        );
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
        tasks.spawn(watch_permissions(Arc::clone(&service)));
        tasks.spawn(manage_capture(
            Arc::clone(&service),
            Arc::clone(&config.backend),
        ));
        tasks.spawn(drive_local_pointer(
            Arc::clone(&service),
            Arc::clone(&config.backend),
        ));

        let runtime = Runtime {
            link: LinkEnv {
                service: Arc::clone(&service),
                backend: Arc::clone(&config.backend),
                timing: LinkTiming::default(),
            },
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

#[derive(Clone)]
struct Runtime {
    link: LinkEnv,
    tasks: Tasks,
}

/// Re-checks the OS input permissions every couple of seconds, so a grant
/// made in System Settings (or `usermod`) shows up without a restart.
async fn watch_permissions(service: Arc<DaemonService>) {
    let mut poll = tokio::time::interval(Duration::from_secs(2));
    loop {
        poll.tick().await;
        service.refresh_permissions().await;
    }
}

/// Captures physical input while, and only while, this device is a
/// controller: a controlled device never captures for forwarding. The
/// router is the capture sink, so one capture serves hotkeys,
/// suppression and routing.
async fn manage_capture(service: Arc<DaemonService>, backend: Arc<dyn PlatformBackend>) {
    let mut settings = service.watch_settings();
    let mut permission = service.watch_permission();
    let mut capture: Option<CaptureGuard> = None;
    loop {
        let controller = settings.borrow_and_update().role == Role::Controller;
        if controller && capture.is_none() {
            let sink = Arc::clone(service.router()) as Arc<dyn flow_core::input::CaptureSink>;
            match backend.start_capture(sink) {
                Ok(guard) => capture = Some(guard),
                Err(err) => crate::logging::product::error(
                    "input capture would not start; this device cannot share its keyboard and mouse",
                    &err,
                ),
            }
        } else if !controller {
            capture = None;
        }
        // Wake on a role change, or when a missing permission is granted
        // (a failed capture is retried then).
        tokio::select! {
            changed = settings.changed() => if changed.is_err() { return },
            changed = permission.changed() => if changed.is_err() { return },
        }
    }
}

/// The controller's own pointer: reports where it is so the router can
/// switch at a screen edge, and moves it when an edge switch comes home.
/// Backends that can't read the pointer simply never trigger edge
/// switching from the local side.
async fn drive_local_pointer(service: Arc<DaemonService>, backend: Arc<dyn PlatformBackend>) {
    let (warp_tx, mut warps) = tokio::sync::mpsc::unbounded_channel();
    service.router().set_local_warp(warp_tx);
    let mut injector = None;
    let mut poll = tokio::time::interval(Duration::from_millis(25));
    loop {
        tokio::select! {
            Some((x, y)) = warps.recv() => {
                if injector.is_none() {
                    injector = backend.new_injector().ok();
                }
                if let Some(injector) = injector.as_mut() {
                    if let Err(err) = injector.warp_cursor(x, y) {
                        crate::logging::product::error("move the local pointer", &err);
                    }
                }
            }
            _ = poll.tick() => {
                if service.local_role() == Role::Controller {
                    if let Some((x, y)) = backend.cursor_position() {
                        service.router().local_cursor(x, y);
                    }
                }
            }
        }
    }
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

        let service = Arc::clone(&self.link.service);
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
        let (name, os) = self.link.service.local_device_identity().await;
        let discovery = match DiscoveryService::bind(
            config.port,
            name,
            os,
            channel_port,
            self.link.service.local_instance_id(),
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
        let announce = config.announce_interval;
        let runtime = self.clone();
        self.tasks.spawn(async move {
            let mut announce_interval = tokio::time::interval(announce);
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
                    received = discovery.recv_announce() => match received {
                        Ok(Some(announce)) => {
                            runtime
                                .handle_discovered_peer(announce.peer, &announce.instance_id)
                                .await
                        }
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
    async fn handle_discovered_peer(&self, peer: DiscoveredPeer, instance_id: &str) {
        crate::hop!(
            stage = "discovered",
            role = "local",
            peer = %peer.name,
            address = ?peer.address,
            "discovery announce parsed for a peer"
        );
        self.link.service.note_discovered_peer(peer.clone()).await;

        // Peers announce for as long as they run; a peer that already has a
        // link needs no new Noise handshake on every announce.
        if self.link.service.has_link_to_instance(instance_id) {
            return;
        }

        let runtime = self.clone();
        self.tasks.spawn(async move {
            let Some((channel, device_id, precedence)) =
                runtime.link.service.dial_if_trusted(peer.address).await
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
            .link
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
    /// runs the link if it wins.
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
        if precedence == ConnectionPrecedence::Redundant {
            let _ = channel.close().await;
            crate::hop_note!(
                stage = "claim_dropped",
                role = "local",
                peer = %device_id.0,
                "dropped this connection as redundant"
            );
            return;
        }
        run_link(&self.link, channel, device_id).await;
    }
}
