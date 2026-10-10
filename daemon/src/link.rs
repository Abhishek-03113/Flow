//! One authenticated daemon-to-daemon connection, from `Hello` to close
//! (`docs/adr/0001-controller-controlled-input-model.md`).
//!
//! Both ends open with `Hello`; a pairing of two controllers or two
//! controlled devices (or a second controller on a controlled device) is
//! refused with `Goodbye{role_conflict}`. After that the controller's end
//! carries out what the router queued (`Enter`/`Leave`/`Input`) and the
//! controlled end injects only between `Enter` and `Leave`.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use flow_core::channel::{Channel, ChannelError, ChannelMessage, PROTOCOL_VERSION};
use flow_core::control::Role;
use flow_core::device::DeviceId;
use flow_core::protocol::{InputEvent, KeyboardEvent, MouseButton, MouseEvent};
use tokio::time::{interval_at, timeout, Instant};

use crate::platform_backend::{Injector, PlatformBackend};
use crate::router::LinkOut;
use crate::service::{DaemonService, LinkRegistration, PeerInfo};

#[derive(Debug, Clone, Copy)]
pub struct LinkTiming {
    pub heartbeat_interval: Duration,
    /// Silence longer than this ends the link.
    pub liveness_timeout: Duration,
    pub hello_timeout: Duration,
}

impl Default for LinkTiming {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(2),
            liveness_timeout: Duration::from_secs(7),
            hello_timeout: Duration::from_secs(5),
        }
    }
}

/// Everything a link needs from the engine.
#[derive(Clone)]
pub struct LinkEnv {
    pub service: Arc<DaemonService>,
    pub backend: Arc<dyn PlatformBackend>,
    pub timing: LinkTiming,
}

/// Runs a link until it ends, then reports it to the service. Dropping
/// the connection on any path releases the peer's slot.
pub async fn run_link(env: &LinkEnv, mut channel: Box<dyn Channel>, device_id: DeviceId) {
    let Some(mut registration) = env.service.register_link(&device_id) else {
        let _ = channel.close().await;
        crate::hop_note!(
            stage = "claim_lost",
            role = "local",
            peer = %device_id.0,
            "another link to this peer already exists; dropping this connection"
        );
        return;
    };
    crate::hop_note!(stage = "link_start", role = "local", peer = %device_id.0, "link starting");
    let error = session(env, channel.as_mut(), &device_id, &mut registration)
        .await
        .err();
    let _ = channel.close().await;
    env.service.link_down(&device_id, error.as_deref()).await;
    crate::hop_note!(
        stage = "link_end",
        role = "local",
        peer = %device_id.0,
        error = error.as_deref().unwrap_or("none"),
        "link ended"
    );
}

/// `Ok` for an ordinary end (peer left, link lost); `Err(reason)` when
/// the link was refused or broken in a way worth showing on the device.
async fn session(
    env: &LinkEnv,
    channel: &mut dyn Channel,
    device_id: &DeviceId,
    registration: &mut LinkRegistration,
) -> Result<(), String> {
    let peer = exchange_hello(env, channel, device_id).await?;
    env.service
        .link_up(device_id, &peer, registration.outbox.clone())
        .await;
    match env.service.local_role() {
        Role::Controller => controller_loop(env, channel, device_id, registration).await,
        Role::Controlled => controlled_loop(env, channel, device_id, registration).await,
    }
}

/// How a link ends when the peer says goodbye: refusals are shown on the
/// device, an orderly leave is not.
fn peer_goodbye(reason: String) -> Result<(), String> {
    match reason.as_str() {
        "role_conflict" | "protocol_mismatch" => Err(reason),
        _ => Ok(()),
    }
}

async fn exchange_hello(
    env: &LinkEnv,
    channel: &mut dyn Channel,
    device_id: &DeviceId,
) -> Result<PeerInfo, String> {
    let (device_name, os, screen, role) = env.service.local_hello().await;
    channel
        .send(ChannelMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            role,
            os,
            screen,
            device_name,
        })
        .await
        .map_err(|_| "connection_lost".to_string())?;

    let received = timeout(env.timing.hello_timeout, async {
        loop {
            match channel.recv().await {
                Ok(ChannelMessage::Heartbeat) => continue,
                other => return other,
            }
        }
    })
    .await
    .map_err(|_| "hello_timeout".to_string())?;

    match received {
        Ok(ChannelMessage::Hello {
            protocol_version,
            role,
            os,
            screen,
            ..
        }) => {
            if protocol_version != PROTOCOL_VERSION {
                return Err(refuse(channel, "protocol_mismatch").await);
            }
            let peer = PeerInfo { role, os, screen };
            match env.service.check_peer(device_id, &peer) {
                Ok(()) => Ok(peer),
                Err(reason) => Err(refuse(channel, reason).await),
            }
        }
        // The peer refused us; surface its reason on our side too.
        Ok(ChannelMessage::Goodbye { reason }) => Err(reason),
        // A v1 peer opens with a message this version can't even parse.
        Err(ChannelError::Serialization(_)) => Err(refuse(channel, "protocol_mismatch").await),
        Ok(_) => Err(refuse(channel, "protocol_error").await),
        Err(_) => Err("connection_lost".to_string()),
    }
}

async fn refuse(channel: &mut dyn Channel, reason: &str) -> String {
    let _ = channel
        .send(ChannelMessage::Goodbye {
            reason: reason.to_string(),
        })
        .await;
    reason.to_string()
}

async fn controller_loop(
    env: &LinkEnv,
    channel: &mut dyn Channel,
    device_id: &DeviceId,
    registration: &mut LinkRegistration,
) -> Result<(), String> {
    let timing = env.timing;
    let mut seq: u64 = 0;
    let mut heartbeat = interval_at(
        Instant::now() + timing.heartbeat_interval,
        timing.heartbeat_interval,
    );
    let mut last_heard = Instant::now();
    loop {
        tokio::select! {
            queued = registration.inbox.recv() => {
                let Some(queued) = queued else { return Ok(()) };
                let message = match queued {
                    LinkOut::Enter { x, y, held_modifiers } => {
                        seq += 1;
                        ChannelMessage::Enter { seq, x, y, held_modifiers }
                    }
                    LinkOut::Leave => {
                        seq += 1;
                        ChannelMessage::Leave { seq }
                    }
                    LinkOut::Input(event) => {
                        seq += 1;
                        ChannelMessage::Input { seq, event }
                    }
                    LinkOut::Close { reason } => {
                        let _ = channel.send(ChannelMessage::Goodbye { reason }).await;
                        return Ok(());
                    }
                    LinkOut::ReleaseRequest => continue,
                };
                if channel.send(message).await.is_err() {
                    return Ok(());
                }
            }
            received = channel.recv() => {
                last_heard = Instant::now();
                match received {
                    Ok(ChannelMessage::ReleaseRequest) => {
                        env.service.router().release_requested(device_id);
                    }
                    Ok(ChannelMessage::ScreenInfo { width, height }) => {
                        env.service
                            .peer_screen_changed(device_id, flow_core::device::ScreenSize { width, height })
                            .await;
                    }
                    Ok(ChannelMessage::Goodbye { reason }) => return peer_goodbye(reason),
                    Err(_) => return Ok(()),
                    // Heartbeats, and anything a controlled peer has no
                    // business sending a controller.
                    Ok(_) => {}
                }
            }
            _ = heartbeat.tick() => {
                if last_heard.elapsed() > timing.liveness_timeout
                    || channel.send(ChannelMessage::Heartbeat).await.is_err()
                {
                    return Ok(());
                }
            }
        }
    }
}

async fn controlled_loop(
    env: &LinkEnv,
    channel: &mut dyn Channel,
    device_id: &DeviceId,
    registration: &mut LinkRegistration,
) -> Result<(), String> {
    let timing = env.timing;
    let mut outcome = Ok(());
    let injector = match env.backend.new_injector() {
        Ok(injector) => injector,
        Err(err) => {
            crate::logging::product::error("input injector would not start", &err);
            return Err("injector_unavailable".to_string());
        }
    };
    let mut endpoint = ControlledEndpoint::new(injector);
    let mut announced_screen = env.backend.screen_size();
    let mut heartbeat = interval_at(
        Instant::now() + timing.heartbeat_interval,
        timing.heartbeat_interval,
    );
    let mut last_heard = Instant::now();
    loop {
        tokio::select! {
            queued = registration.inbox.recv() => {
                match queued {
                    Some(LinkOut::ReleaseRequest) => {
                        if channel.send(ChannelMessage::ReleaseRequest).await.is_err() {
                            break;
                        }
                    }
                    Some(LinkOut::Close { reason }) => {
                        let _ = channel.send(ChannelMessage::Goodbye { reason }).await;
                        break;
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            received = channel.recv() => {
                last_heard = Instant::now();
                match received {
                    Ok(ChannelMessage::Goodbye { reason }) => {
                        outcome = peer_goodbye(reason);
                        break;
                    }
                    Err(_) => break,
                    Ok(message) => {
                        if let Some(entered) = endpoint.handle(&message) {
                            env.service.set_entered(entered);
                        }
                    }
                }
            }
            _ = heartbeat.tick() => {
                if last_heard.elapsed() > timing.liveness_timeout {
                    break;
                }
                let screen = env.backend.screen_size();
                let sent = if screen != announced_screen {
                    announced_screen = screen;
                    channel
                        .send(ChannelMessage::ScreenInfo { width: screen.width, height: screen.height })
                        .await
                } else {
                    channel.send(ChannelMessage::Heartbeat).await
                };
                if sent.is_err() {
                    break;
                }
            }
        }
    }
    // Whatever ended the link, never leave a key or button stuck down.
    endpoint.release_all();
    env.service.set_entered(false);
    crate::hop!(
        stage = "controlled_end",
        role = "receiver",
        peer = %device_id.0,
        dropped = endpoint.dropped,
        "controlled side released everything"
    );
    outcome
}

/// The controlled side's injection gate: input is injected only between
/// `Enter` and `Leave`, replays are dropped, and everything held is
/// released when the controller leaves or goes away.
pub struct ControlledEndpoint {
    injector: Box<dyn Injector>,
    entered: bool,
    last_seq: Option<u64>,
    held_keys: HashSet<String>,
    held_buttons: HashSet<MouseButton>,
    /// Input dropped for arriving outside `Enter`..`Leave` or replayed.
    pub dropped: u64,
}

impl ControlledEndpoint {
    pub fn new(injector: Box<dyn Injector>) -> Self {
        Self {
            injector,
            entered: false,
            last_seq: None,
            held_keys: HashSet::new(),
            held_buttons: HashSet::new(),
            dropped: 0,
        }
    }

    /// Applies one message from the controller. Returns the new entered
    /// state when `message` changed it.
    pub fn handle(&mut self, message: &ChannelMessage) -> Option<bool> {
        match message {
            ChannelMessage::Enter { seq, x, y, .. } if self.accept_seq(*seq) => {
                self.entered = true;
                if let Err(err) = self.injector.warp_cursor(*x, *y) {
                    crate::logging::product::error("move the cursor to the entry point", &err);
                }
                Some(true)
            }
            ChannelMessage::Leave { seq } if self.accept_seq(*seq) => {
                self.release_all();
                self.entered = false;
                Some(false)
            }
            ChannelMessage::Input { seq, event } => {
                if self.entered && self.accept_seq(*seq) {
                    self.inject(event);
                } else {
                    self.dropped += 1;
                }
                None
            }
            _ => None,
        }
    }

    /// Replay guard: only a sequence above the last accepted one counts.
    fn accept_seq(&mut self, seq: u64) -> bool {
        if self.last_seq.is_some_and(|last| seq <= last) {
            self.dropped += 1;
            return false;
        }
        self.last_seq = Some(seq);
        true
    }

    fn inject(&mut self, event: &InputEvent) {
        match self.injector.inject(event) {
            Ok(()) => match event {
                InputEvent::Keyboard(KeyboardEvent::KeyDown { key, .. }) => {
                    self.held_keys.insert(key.clone());
                }
                InputEvent::Keyboard(KeyboardEvent::KeyUp { key, .. }) => {
                    self.held_keys.remove(key);
                }
                InputEvent::Mouse(MouseEvent::ButtonDown { button, .. }) => {
                    self.held_buttons.insert(*button);
                }
                InputEvent::Mouse(MouseEvent::ButtonUp { button, .. }) => {
                    self.held_buttons.remove(button);
                }
                InputEvent::Mouse(_) => {}
            },
            Err(err) => crate::logging::product::error("inject input", &err),
        }
    }

    /// Synthesises a release for every key and button still held.
    pub fn release_all(&mut self) {
        let timestamp_ms = now_ms();
        for key in std::mem::take(&mut self.held_keys) {
            let release = InputEvent::Keyboard(KeyboardEvent::KeyUp {
                key,
                modifiers: Vec::new(),
                timestamp_ms,
            });
            if let Err(err) = self.injector.inject(&release) {
                crate::logging::product::error("release a held key", &err);
            }
        }
        for button in std::mem::take(&mut self.held_buttons) {
            let release = InputEvent::Mouse(MouseEvent::ButtonUp {
                button,
                timestamp_ms,
            });
            if let Err(err) = self.injector.inject(&release) {
                crate::logging::product::error("release a held button", &err);
            }
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
