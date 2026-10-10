//! A set of in-process engines ("virtual devices"), isolated from each
//! other and the host: ephemeral ports, explicit discovery targets and a
//! temp data dir each. They find each other through the real discovery and
//! pairing flow and talk the real v2 protocol over loopback; only the
//! operating system underneath is virtual ([`VirtualBackend`]).

use std::collections::BTreeSet;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use flow_core::control::Role;
use flow_core::device::{Device, DeviceId, DeviceState, HostOs, ScreenSize, LOCAL_DEVICE_ID};
use flow_core::input::Disposition;
use flow_core::pairing::{PairingDecision, PairingStage};
use flow_core::protocol::{InputEvent, KeyboardEvent, Modifier, MouseButton, MouseEvent};
use flow_core::switch_key::SwitchKeyBinding;
use flow_daemon::devmode::SecurityMode;
use flow_daemon::engine::{DiscoveryConfig, Engine, EngineConfig, EngineHandle, IpcConfig};
use flow_daemon::platform_backend::PlatformBackend;
use flow_daemon::service::{DaemonService, IpcClientGuard};
use flow_daemon::virtual_backend::VirtualBackend;
use tokio::task::JoinHandle;

use crate::keys::{modifier_of, OsKeyboard};

/// One machine in a world.
#[derive(Debug, Clone)]
pub struct DeviceSpec {
    pub name: String,
    pub os: HostOs,
    pub role: Role,
    pub screen: ScreenSize,
}

/// What a link between two devices is expected to become.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkExpect {
    Live,
    /// Refused with this `link_error`, e.g. `role_conflict`.
    Refused(String),
}

#[derive(Debug, Clone)]
pub struct LinkSpec {
    /// The side that initiates pairing.
    pub a: String,
    pub b: String,
    pub expect: LinkExpect,
}

/// Where each engine's control server listens, if it has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcMode {
    Off,
    Ephemeral,
    /// Device `i` listens on `base + i`.
    Base(u16),
}

#[derive(Debug, Clone)]
pub struct WorldOptions {
    pub security: SecurityMode,
    /// Hotkey debounce; zero lets scenarios switch faster than a human.
    pub debounce: Duration,
    /// How long nothing may change before the world counts as quiet.
    pub settle: Duration,
    /// Deadline for every wait during a scenario.
    pub timeout: Duration,
    /// Deadline for starting engines, pairing and linking (slow when many worlds share a CPU).
    pub setup_timeout: Duration,
    pub ipc: IpcMode,
    pub test_hooks: bool,
}

impl Default for WorldOptions {
    fn default() -> Self {
        Self {
            security: SecurityMode::Secure,
            debounce: Duration::ZERO,
            settle: Duration::from_millis(50),
            timeout: Duration::from_secs(10),
            setup_timeout: Duration::from_secs(40),
            ipc: IpcMode::Off,
            test_hooks: false,
        }
    }
}

/// A physical event the harness fed to a device, kept to trace where it
/// ended up.
struct Fed {
    timestamp_ms: u64,
}

pub struct VirtualDevice {
    pub spec: DeviceSpec,
    pub backend: Arc<VirtualBackend>,
    pub keyboard: OsKeyboard,
    dir: tempfile::TempDir,
    engine: Option<EngineHandle>,
    /// Pretends a UI is attached (an unattended daemon refuses pairing).
    ui_guard: Option<IpcClientGuard>,
    consent: Option<JoinHandle<()>>,
    held_modifiers: BTreeSet<String>,
    /// Where its pointer starts (the screen centre).
    pointer: (i32, i32),
    fed: Vec<Fed>,
}

impl VirtualDevice {
    fn service(&self) -> Result<&Arc<DaemonService>, String> {
        self.engine
            .as_ref()
            .map(EngineHandle::service)
            .ok_or_else(|| format!("{} is offline", self.spec.name))
    }

    /// Where this device's control server listens (when it has one).
    pub fn ipc_addr(&self) -> Option<SocketAddr> {
        self.engine.as_ref().and_then(EngineHandle::ipc_addr)
    }

    pub fn token(&self) -> Option<String> {
        std::fs::read_to_string(self.dir.path().join("ipc.token"))
            .ok()
            .map(|t| t.trim().to_string())
    }
}

impl Drop for World {
    fn drop(&mut self) {
        for device in &mut self.devices {
            if let Some(task) = device.consent.take() {
                task.abort();
            }
            device.ui_guard = None;
            if let Some(engine) = device.engine.take() {
                engine.shutdown();
            }
        }
    }
}

pub struct World {
    devices: Vec<VirtualDevice>,
    links: Vec<LinkSpec>,
    opts: WorldOptions,
    clock: u64,
}

impl World {
    /// Starts every device, then pairs and links them in `links` order.
    pub async fn start(
        specs: Vec<DeviceSpec>,
        links: Vec<LinkSpec>,
        opts: WorldOptions,
    ) -> Result<Self, String> {
        let mut world = Self::start_unlinked(specs, opts).await?;
        for link in &links {
            world.link(link).await?;
        }
        world.links = links;
        Ok(world)
    }

    /// Starts the devices and sets their roles, with no links yet.
    pub async fn start_unlinked(
        specs: Vec<DeviceSpec>,
        opts: WorldOptions,
    ) -> Result<Self, String> {
        let mut seen = BTreeSet::new();
        for spec in &specs {
            if !seen.insert(spec.name.clone()) {
                return Err(format!("two devices are named {:?}", spec.name));
            }
        }
        let devices = specs
            .into_iter()
            .map(|spec| {
                let backend = Arc::new(VirtualBackend::new(spec.os, spec.screen));
                let centre = (spec.screen.width as i32 / 2, spec.screen.height as i32 / 2);
                backend.set_cursor(Some(centre));
                Ok(VirtualDevice {
                    backend,
                    keyboard: OsKeyboard::new(spec.os),
                    dir: tempfile::tempdir().map_err(|e| format!("temp dir: {e}"))?,
                    engine: None,
                    ui_guard: None,
                    consent: None,
                    held_modifiers: BTreeSet::new(),
                    pointer: centre,
                    fed: Vec::new(),
                    spec,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut world = Self {
            devices,
            links: Vec::new(),
            opts,
            clock: 0,
        };
        for i in 0..world.devices.len() {
            world.start_engine(i).await?;
            let service = world.devices[i].service()?.clone();
            let spec = world.devices[i].spec.clone();
            if spec.role != Role::Controller {
                service
                    .set_role(spec.role)
                    .await
                    .map_err(|e| format!("{}: set role: {e}", spec.name))?;
            }
            // A keyboard without Scroll Lock (a Mac's) uses F13, as a user would.
            if spec.role == Role::Controller
                && world.devices[i].keyboard.native_code("SCROLLLOCK").is_err()
            {
                let f13 = flow_core::switch_key::presets()[2].clone();
                service
                    .set_switch_key(f13)
                    .await
                    .map_err(|e| format!("{}: set switch key: {e}", spec.name))?;
            }
            world.await_capture(i).await?;
        }
        Ok(world)
    }

    pub fn devices(&self) -> &[VirtualDevice] {
        &self.devices
    }

    pub fn options(&self) -> &WorldOptions {
        &self.opts
    }

    pub fn index(&self, name: &str) -> Result<usize, String> {
        self.devices
            .iter()
            .position(|d| d.spec.name == name)
            .ok_or_else(|| format!("no device named {name:?}"))
    }

    pub fn device(&self, name: &str) -> Result<&VirtualDevice, String> {
        Ok(&self.devices[self.index(name)?])
    }

    pub fn service(&self, name: &str) -> Result<Arc<DaemonService>, String> {
        Ok(Arc::clone(self.device(name)?.service()?))
    }

    // ---- engines -----------------------------------------------------------

    async fn start_engine(&mut self, i: usize) -> Result<(), String> {
        let ipc = match self.opts.ipc {
            IpcMode::Off => None,
            IpcMode::Ephemeral => Some(0),
            IpcMode::Base(base) => Some(base + i as u16),
        };
        let device = &mut self.devices[i];
        let mut config = EngineConfig::new(device.dir.path().join("data"), device.backend.clone());
        config.device_name = Some(device.spec.name.clone());
        config.security = self.opts.security;
        config.test_hooks = self.opts.test_hooks;
        config.discovery = Some(DiscoveryConfig {
            port: 0,
            targets: Some(Vec::new()),
            announce_interval: Duration::from_millis(250),
        });
        config.ipc = ipc.map(|port| IpcConfig {
            port,
            token_path: device.dir.path().join("ipc.token"),
        });
        let engine = Engine::start(config)
            .await
            .map_err(|e| format!("{}: engine start: {e}", device.spec.name))?;
        let service = Arc::clone(engine.service());
        service.router().set_debounce_window(self.opts.debounce);
        device.ui_guard = Some(service.register_ipc_client());
        device.consent = Some(tokio::spawn(accept_every_pairing_request(service)));
        device.engine = Some(engine);
        Ok(())
    }

    /// Waits until the device's capture matches its role (a controller
    /// captures, a controlled device never does).
    async fn await_capture(&self, i: usize) -> Result<(), String> {
        let device = &self.devices[i];
        let service = device.service()?.clone();
        let backend = Arc::clone(&device.backend);
        wait_until(
            &format!("{} capture to match its role", device.spec.name),
            self.opts.setup_timeout,
            move || backend.is_capturing() == (service.local_role() == Role::Controller),
        )
        .await
    }

    /// Kills the device's engine abruptly: sockets close, nothing says goodbye.
    pub async fn disconnect(&mut self, name: &str) -> Result<(), String> {
        let i = self.index(name)?;
        let device = &mut self.devices[i];
        if let Some(task) = device.consent.take() {
            task.abort();
        }
        device.ui_guard = None;
        match device.engine.take() {
            Some(engine) => engine.shutdown(),
            None => return Err(format!("{name} is already offline")),
        }
        device.held_modifiers.clear();
        Ok(())
    }

    /// Restarts a disconnected device on its old data (so it is still
    /// paired) and waits for its links to come back.
    pub async fn reconnect(&mut self, name: &str) -> Result<(), String> {
        let i = self.index(name)?;
        if self.devices[i].engine.is_some() {
            return Err(format!("{name} is still online"));
        }
        self.start_engine(i).await?;
        self.await_capture(i).await?;
        let links: Vec<LinkSpec> = self
            .links
            .iter()
            .filter(|l| l.a == name || l.b == name)
            .cloned()
            .collect();
        for link in links {
            let (a, b) = (self.index(&link.a)?, self.index(&link.b)?);
            if self.devices[a].engine.is_some() && self.devices[b].engine.is_some() {
                self.connect_discovery(a, b)?;
                self.await_link(&link).await?;
            }
        }
        Ok(())
    }

    /// Stops every engine (also what dropping the world does).
    pub fn shutdown(self) {
        drop(self);
    }

    // ---- pairing and links -------------------------------------------------

    fn discovery_addr(&self, i: usize) -> Result<SocketAddr, String> {
        let port = self.devices[i]
            .engine
            .as_ref()
            .and_then(EngineHandle::discovery_port)
            .ok_or_else(|| format!("{} has no discovery port", self.devices[i].spec.name))?;
        Ok(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port))
    }

    /// Makes two engines announce to each other (and only each other).
    fn connect_discovery(&self, a: usize, b: usize) -> Result<(), String> {
        let (addr_a, addr_b) = (self.discovery_addr(a)?, self.discovery_addr(b)?);
        for (device, target) in [(a, addr_b), (b, addr_a)] {
            self.devices[device]
                .engine
                .as_ref()
                .ok_or("offline")?
                .add_discovery_target(target);
        }
        Ok(())
    }

    /// Pairs `link.a` with `link.b` through discovery and the pairing
    /// flow (the initiator picks the peer from its candidates; the other
    /// side's attached "UI" accepts), then waits for the expected link.
    pub async fn link(&mut self, link: &LinkSpec) -> Result<(), String> {
        let (a, b) = (self.index(&link.a)?, self.index(&link.b)?);
        self.connect_discovery(a, b)?;
        let service = self.devices[a].service()?.clone();
        let target = link.b.clone();

        // The previous link's session may still be settling to idle.
        let _ = service.cancel_pairing().await;
        service
            .start_pairing()
            .await
            .map_err(|e| format!("{}: start pairing: {e}", link.a))?;
        let mut candidate = None;
        let session = service.watch_pairing_session();
        wait_until(
            &format!("{} to discover {}", link.a, link.b),
            self.opts.setup_timeout,
            || {
                let current = session.borrow();
                candidate = (current.stage == PairingStage::Found)
                    .then(|| current.candidates.iter().find(|c| c.name == target))
                    .flatten()
                    .map(|c| c.id.clone());
                candidate.is_some()
            },
        )
        .await?;
        service
            .pair_with_candidate(&candidate.unwrap_or_default())
            .await
            .map_err(|e| format!("{}: pair with {}: {e}", link.a, link.b))?;
        self.await_link(link).await
    }

    /// Waits until both ends show the link the way `link.expect` says.
    async fn await_link(&self, link: &LinkSpec) -> Result<(), String> {
        let (a, b) = (self.index(&link.a)?, self.index(&link.b)?);
        let sides = [(a, link.b.clone()), (b, link.a.clone())];
        let watches: Vec<_> = sides
            .iter()
            .map(|(i, peer)| Ok((self.devices[*i].service()?.watch_devices(), peer.clone())))
            .collect::<Result<_, String>>()?;
        let expect = link.expect.clone();
        wait_until(
            &format!("{} <-> {} to be {:?}", link.a, link.b, link.expect),
            self.opts.setup_timeout,
            move || {
                watches.iter().all(|(devices, peer)| {
                    let devices = devices.borrow();
                    let Some(device) = devices.iter().find(|d| &d.name == peer) else {
                        return false;
                    };
                    match &expect {
                        LinkExpect::Live => is_live(device),
                        LinkExpect::Refused(reason) => device.link_error.as_deref() == Some(reason),
                    }
                })
            },
        )
        .await
    }

    // ---- physical input ----------------------------------------------------

    fn next_timestamp(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// Hands one physical event to the device's capture, as its OS would.
    fn feed(&mut self, i: usize, event: InputEvent) -> Option<Disposition> {
        let timestamp_ms = event.timestamp_ms();
        let device = &mut self.devices[i];
        device.fed.push(Fed { timestamp_ms });
        device.backend.feed(event)
    }

    /// Presses or releases the key the OS's keyboard calls `name`.
    pub fn key(&mut self, name: &str, key: &str, down: bool) -> Result<(), String> {
        let i = self.index(name)?;
        let timestamp_ms = self.next_timestamp();
        let device = &mut self.devices[i];
        // Native code in, wire name out: exactly what the capture adapter does.
        let code = device.keyboard.native_code(key)?;
        let wire = device.keyboard.captured_name(code);
        if modifier_of(&wire).is_some() {
            if down {
                device.held_modifiers.insert(wire.clone());
            } else {
                device.held_modifiers.remove(&wire);
            }
        }
        let modifiers = modifiers_of(&device.held_modifiers);
        let event = InputEvent::Keyboard(if down {
            KeyboardEvent::KeyDown {
                key: wire,
                modifiers,
                timestamp_ms,
            }
        } else {
            KeyboardEvent::KeyUp {
                key: wire,
                modifiers,
                timestamp_ms,
            }
        });
        self.feed(i, event);
        Ok(())
    }

    pub fn button(&mut self, name: &str, button: MouseButton, down: bool) -> Result<(), String> {
        let i = self.index(name)?;
        let timestamp_ms = self.next_timestamp();
        self.feed(
            i,
            InputEvent::Mouse(if down {
                MouseEvent::ButtonDown {
                    button,
                    timestamp_ms,
                }
            } else {
                MouseEvent::ButtonUp {
                    button,
                    timestamp_ms,
                }
            }),
        );
        Ok(())
    }

    pub fn scroll(&mut self, name: &str, dx: i32, dy: i32) -> Result<(), String> {
        let i = self.index(name)?;
        let timestamp_ms = self.next_timestamp();
        self.feed(
            i,
            InputEvent::Mouse(MouseEvent::Scroll {
                dx,
                dy,
                timestamp_ms,
            }),
        );
        Ok(())
    }

    /// Moves the pointer. While the device's own screen is active the
    /// virtual pointer follows (the engine watches it for edge switching).
    pub fn pointer_move(&mut self, name: &str, dx: i32, dy: i32) -> Result<(), String> {
        let i = self.index(name)?;
        let timestamp_ms = self.next_timestamp();
        let disposition = self.feed(
            i,
            InputEvent::Mouse(MouseEvent::Move {
                dx,
                dy,
                timestamp_ms,
            }),
        );
        if disposition == Some(Disposition::Pass) {
            let device = &self.devices[i];
            let screen = device.spec.screen;
            let (x, y) = device.backend.cursor_position().unwrap_or(device.pointer);
            device.backend.set_cursor(Some((
                (x + dx).clamp(0, screen.width as i32 - 1),
                (y + dy).clamp(0, screen.height as i32 - 1),
            )));
        }
        Ok(())
    }

    /// Puts the device's own pointer at (x, y), as if the user dragged it there.
    pub fn place_cursor(&mut self, name: &str, x: i32, y: i32) -> Result<(), String> {
        let i = self.index(name)?;
        self.devices[i].backend.set_cursor(Some((x, y)));
        Ok(())
    }

    /// Forgets recorded input on `name` (or every device), to start a
    /// fresh observation window.
    pub fn clear(&mut self, name: Option<&str>) -> Result<(), String> {
        let only = name.map(|n| self.index(n)).transpose()?;
        for (i, device) in self.devices.iter_mut().enumerate() {
            if only.is_none_or(|o| o == i) {
                device.backend.clear_logs();
                device.fed.clear();
            }
        }
        Ok(())
    }

    // ---- engine commands and state ---------------------------------------

    /// The id under which `observer` knows the device called `peer`.
    pub fn id_of(&self, observer: &str, peer: &str) -> Result<DeviceId, String> {
        if peer == "local" {
            return Ok(DeviceId(LOCAL_DEVICE_ID.to_string()));
        }
        self.devices_of(observer)?
            .into_iter()
            .find(|d| d.name == peer)
            .map(|d| d.id)
            .ok_or_else(|| format!("{observer} does not know a device named {peer:?}"))
    }

    pub fn devices_of(&self, observer: &str) -> Result<Vec<Device>, String> {
        Ok(self
            .device(observer)?
            .service()?
            .watch_devices()
            .borrow()
            .clone())
    }

    /// The controller's active screen: `local` or the remote's name.
    pub fn active_screen(&self, name: &str) -> Result<String, String> {
        let service = self.service(name)?;
        let active = service.router().active();
        Ok(match active.remote() {
            None => "local".to_string(),
            Some(id) => self
                .devices_of(name)?
                .into_iter()
                .find(|d| &d.id == id)
                .map_or_else(|| id.0.clone(), |d| d.name),
        })
    }

    /// The switch-key (or return-home) chord, as keys to press in order.
    pub fn hotkey_chord(&self, name: &str, return_home: bool) -> Result<Vec<String>, String> {
        let settings = self.service(name)?.watch_settings().borrow().clone();
        let binding: SwitchKeyBinding = if return_home {
            settings
                .return_home_key
                .ok_or_else(|| format!("{name} has no return-home key configured"))?
        } else {
            settings.switch_key
        };
        Ok(binding
            .keys
            .iter()
            .map(|t| crate::keys::chord_key(t))
            .collect())
    }

    // ---- observation -------------------------------------------------------

    /// A cheap fingerprint of everything observable, to tell when the
    /// world has gone quiet.
    fn fingerprint(&self) -> Vec<String> {
        self.devices
            .iter()
            .map(|d| {
                let (delivered, injected, warps) = d.backend.counts();
                let (active, entered) = d.engine.as_ref().map_or_else(
                    || (String::new(), false),
                    |e| {
                        let service = e.service();
                        let control = service.watch_control_state().borrow().clone();
                        (format!("{:?}", service.router().active()), control.entered)
                    },
                );
                format!("{delivered}/{injected}/{warps}/{active}/{entered}")
            })
            .collect()
    }

    /// Waits until nothing observable has changed for the settle window,
    /// so "nothing happened" can be asserted without a fixed sleep.
    pub async fn settle(&self) {
        let mut last = self.fingerprint();
        let mut since = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_millis(3)).await;
            let now = self.fingerprint();
            if now != last {
                last = now;
                since = Instant::now();
            } else if since.elapsed() >= self.opts.settle {
                return;
            }
        }
    }

    /// The timestamps of physical events fed to `name` since the last clear.
    pub fn fed_timestamps(&self, name: &str) -> Result<Vec<u64>, String> {
        Ok(self
            .device(name)?
            .fed
            .iter()
            .map(|f| f.timestamp_ms)
            .collect())
    }
}

/// Whether a device in the list has a live, accepted link (`role` is only
/// known once its `Hello` was accepted).
pub fn is_live(device: &Device) -> bool {
    device.role.is_some()
        && device.link_error.is_none()
        && matches!(device.state, DeviceState::Inactive | DeviceState::Active)
}

fn modifiers_of(held: &BTreeSet<String>) -> Vec<Modifier> {
    let mut out = Vec::new();
    for key in held {
        if let Some(modifier) = modifier_of(key) {
            if !out.contains(&modifier) {
                out.push(modifier);
            }
        }
    }
    out
}

/// The "user" on a device clicks Accept on every incoming pairing request.
async fn accept_every_pairing_request(service: Arc<DaemonService>) {
    let mut requests = service.watch_incoming_request();
    loop {
        let pending = requests.borrow_and_update().clone();
        if let Some(request) = pending {
            let _ = service
                .respond_to_pairing_request(&request.request_id, PairingDecision::Accept)
                .await;
        }
        if requests.changed().await.is_err() {
            return;
        }
    }
}

/// Polls `condition` every few milliseconds until it holds or `timeout`
/// passes. The one waiting primitive: no fixed sleeps anywhere.
pub async fn wait_until(
    what: &str,
    timeout: Duration,
    mut condition: impl FnMut() -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if condition() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("timed out after {timeout:?} waiting for {what}"));
        }
        tokio::time::sleep(Duration::from_millis(4)).await;
    }
}
