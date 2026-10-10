//! The controller's single point of input control
//! (`docs/adr/0001-controller-controlled-input-model.md`).
//!
//! One [`InputRouter`] owns the process's only capture. It sees every
//! physical event — also while input is hidden from local applications —
//! so the switch hotkey keeps working and returning home always works. For
//! each event it decides, synchronously and under one lock, where the event
//! goes and whether local applications may see it, which is what makes the
//! leak invariants hold:
//!
//! - suppression is switched on *before* `Enter` is sent and off only
//!   *after* `Leave`;
//! - every key and button release follows its press ("up follows down"),
//!   whatever screen is active when it happens;
//! - the hotkey chord is swallowed and never forwarded;
//! - link loss while a peer is active returns home.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use flow_core::control::ActiveScreen;
use flow_core::device::{DeviceId, HostOs, ScreenSize};
use flow_core::input::{CaptureSink, Disposition};
use flow_core::keymap::remap::{apply_remaps, RemapRule};
use flow_core::protocol::{InputEvent, KeyboardEvent, Modifier, MouseButton, MouseEvent};
use flow_core::settings::{FlowSettings, PointerSensitivity};
use thiserror::Error;
use tokio::sync::{mpsc, watch};

use crate::hotkey::debounce::{SwitchDebouncer, SWITCH_DEBOUNCE_WINDOW};
use crate::hotkey::SwitchKeyMatcher;

/// What a link task writes to its peer on the router's behalf. The task
/// stamps sequence numbers, so ordering here is wire ordering.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkOut {
    Enter {
        x: i32,
        y: i32,
        held_modifiers: Vec<Modifier>,
    },
    Leave,
    Input(InputEvent),
    /// Controlled side only: ask the controller for this machine back.
    ReleaseRequest,
    /// Orderly close of the link, with a reason for `Goodbye`.
    Close {
        reason: String,
    },
}

/// A connected controlled peer the router can switch to.
#[derive(Debug, Clone)]
pub struct PeerSlot {
    pub id: DeviceId,
    pub os: HostOs,
    pub screen: ScreenSize,
    pub outbox: mpsc::UnboundedSender<LinkOut>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SwitchError {
    #[error("no live link to {0:?}")]
    NotConnected(DeviceId),
}

/// A physical key or button, tracked from press to release.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Held {
    Key(String),
    Button(MouseButton),
}

/// Where the release of a held key or button must go.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Route {
    Local,
    Remote(DeviceId),
    /// Pressed while hidden from local applications, but the remote it
    /// went to has since released everything: swallow the release.
    Swallow,
}

/// Counters behind the `Diagnostics` the service publishes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RouterStats {
    pub events_forwarded: u64,
    pub events_dropped: u64,
    /// Time from a switch request to `Enter` being queued, most recent last.
    pub switch_latencies_ms: Vec<f64>,
}

const MAX_LATENCY_SAMPLES: usize = 256;

struct State {
    peers: Vec<PeerSlot>,
    active: ActiveScreen,
    switch_key: SwitchKeyMatcher,
    return_home_key: Option<SwitchKeyMatcher>,
    debouncer: SwitchDebouncer,
    held: HashMap<Held, Route>,
    /// Modifiers reported by the most recent keyboard event, sent with
    /// `Enter` so the target knows what is down.
    modifiers: Vec<Modifier>,
    share_keyboard: bool,
    share_mouse: bool,
    pointer_scale: f32,
    /// Fractional pointer movement carried between scaled deltas.
    pointer_remainder: (f32, f32),
    remap_rules: Vec<RemapRule>,
    events_forwarded: u64,
    events_dropped: u64,
    switch_latencies_ms: VecDeque<f64>,
}

pub struct InputRouter {
    state: Mutex<State>,
    /// Mirrors "active is remote" for adapters that need it without the lock.
    suppressing: AtomicBool,
    active_tx: watch::Sender<ActiveScreen>,
}

fn pointer_scale(sensitivity: PointerSensitivity) -> f32 {
    match sensitivity {
        PointerSensitivity::Low => 0.6,
        PointerSensitivity::Normal => 1.0,
        PointerSensitivity::High => 1.6,
    }
}

impl InputRouter {
    pub fn new(settings: &FlowSettings, remap_rules: Vec<RemapRule>) -> Self {
        let (active_tx, _) = watch::channel(ActiveScreen::Local);
        let router = Self {
            state: Mutex::new(State {
                peers: Vec::new(),
                active: ActiveScreen::Local,
                switch_key: SwitchKeyMatcher::new(settings.switch_key.clone()),
                return_home_key: None,
                debouncer: SwitchDebouncer::new(SWITCH_DEBOUNCE_WINDOW),
                held: HashMap::new(),
                modifiers: Vec::new(),
                share_keyboard: true,
                share_mouse: true,
                pointer_scale: 1.0,
                pointer_remainder: (0.0, 0.0),
                remap_rules,
                events_forwarded: 0,
                events_dropped: 0,
                switch_latencies_ms: VecDeque::new(),
            }),
            suppressing: AtomicBool::new(false),
            active_tx,
        };
        router.apply_settings(settings);
        router
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Re-reads everything the router takes from settings. Clears matcher
    /// state, as a chord spanning a rebind matches neither binding.
    pub fn apply_settings(&self, settings: &FlowSettings) {
        let mut st = self.lock();
        st.switch_key.set_binding(settings.switch_key.clone());
        st.return_home_key = settings.return_home_key.clone().map(SwitchKeyMatcher::new);
        st.share_keyboard = settings.share_keyboard;
        st.share_mouse = settings.share_mouse;
        st.pointer_scale = pointer_scale(settings.pointer_sensitivity);
    }

    pub fn set_remap_rules(&self, rules: Vec<RemapRule>) {
        self.lock().remap_rules = rules;
    }

    /// Replaces the debounce window; tests use zero.
    pub fn set_debounce_window(&self, window: Duration) {
        self.lock().debouncer = SwitchDebouncer::new(window);
    }

    pub fn watch_active(&self) -> watch::Receiver<ActiveScreen> {
        self.active_tx.subscribe()
    }

    pub fn active(&self) -> ActiveScreen {
        self.lock().active.clone()
    }

    /// Ids of the peers that can be switched to, in switching order.
    pub fn peer_ids(&self) -> Vec<DeviceId> {
        self.lock().peers.iter().map(|p| p.id.clone()).collect()
    }

    pub fn stats(&self) -> RouterStats {
        let st = self.lock();
        RouterStats {
            events_forwarded: st.events_forwarded,
            events_dropped: st.events_dropped,
            switch_latencies_ms: st.switch_latencies_ms.iter().copied().collect(),
        }
    }

    // ---- peers ---------------------------------------------------------

    /// Registers a live link. A reconnect replaces the earlier slot.
    pub fn peer_connected(&self, peer: PeerSlot) {
        let mut st = self.lock();
        st.peers.retain(|p| p.id != peer.id);
        st.peers.push(peer);
    }

    /// The link to `id` is gone. Returns home first when it was active, so
    /// the keyboard is never left suppressed for a peer that can't hear it.
    pub fn peer_disconnected(&self, id: &DeviceId) {
        let mut st = self.lock();
        if st.active.remote() == Some(id) {
            // No `Leave`: nobody is listening, and the controlled side
            // releases everything itself when its link ends.
            self.apply_switch(&mut st, ActiveScreen::Local, false);
        }
        for route in st.held.values_mut() {
            if *route == Route::Remote(id.clone()) {
                *route = Route::Swallow;
            }
        }
        st.peers.retain(|p| &p.id != id);
    }

    /// The controlled peer asked for its machine back.
    pub fn release_requested(&self, id: &DeviceId) {
        let mut st = self.lock();
        if st.active.remote() == Some(id) {
            self.apply_switch(&mut st, ActiveScreen::Local, true);
        }
    }

    pub fn peer_screen_changed(&self, id: &DeviceId, screen: ScreenSize) {
        if let Some(peer) = self.lock().peers.iter_mut().find(|p| &p.id == id) {
            peer.screen = screen;
        }
    }

    // ---- switching -----------------------------------------------------

    /// Makes `target` the active screen.
    pub fn switch_to(&self, target: ActiveScreen) -> Result<(), SwitchError> {
        let mut st = self.lock();
        let started = Instant::now();
        self.switch_locked(&mut st, target, started)
    }

    fn switch_locked(
        &self,
        st: &mut State,
        target: ActiveScreen,
        started: Instant,
    ) -> Result<(), SwitchError> {
        if let ActiveScreen::Remote(id) = &target {
            if !st.peers.iter().any(|p| &p.id == id) {
                return Err(SwitchError::NotConnected(id.clone()));
            }
        }
        if st.active == target {
            return Ok(());
        }
        self.apply_switch(st, target, true);
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        if st.switch_latencies_ms.len() == MAX_LATENCY_SAMPLES {
            st.switch_latencies_ms.pop_front();
        }
        st.switch_latencies_ms.push_back(elapsed);
        Ok(())
    }

    /// The transition itself; `target` must be valid. `notify_old` is false
    /// when the previous remote is already gone.
    fn apply_switch(&self, st: &mut State, target: ActiveScreen, notify_old: bool) {
        let old = std::mem::replace(&mut st.active, target.clone());

        // Leave the old screen: the remote releases everything it holds,
        // so releases still pending for it are swallowed later.
        if let ActiveScreen::Remote(old_id) = &old {
            if notify_old {
                send(st, old_id, LinkOut::Leave);
            }
            for route in st.held.values_mut() {
                if *route == Route::Remote(old_id.clone()) {
                    *route = Route::Swallow;
                }
            }
        }

        // Enter the new one. Suppression goes on before `Enter` is queued
        // and off only after `Leave` was.
        match &target {
            ActiveScreen::Remote(id) => {
                self.suppressing.store(true, Ordering::SeqCst);
                let (x, y) = entry_point(st, id);
                let held_modifiers = st.modifiers.clone();
                send(
                    st,
                    id,
                    LinkOut::Enter {
                        x,
                        y,
                        held_modifiers,
                    },
                );
            }
            ActiveScreen::Local => self.suppressing.store(false, Ordering::SeqCst),
        }
        self.active_tx.send_replace(target);
    }

    /// The screen after the active one in `Local -> peers... -> Local` order.
    fn next_screen(st: &State) -> Option<ActiveScreen> {
        let screens: Vec<ActiveScreen> = std::iter::once(ActiveScreen::Local)
            .chain(st.peers.iter().map(|p| ActiveScreen::Remote(p.id.clone())))
            .collect();
        if screens.len() < 2 {
            return None;
        }
        let at = screens.iter().position(|s| *s == st.active).unwrap_or(0);
        Some(screens[(at + 1) % screens.len()].clone())
    }

    // ---- the capture path ----------------------------------------------

    fn route_event(&self, st: &mut State, event: &InputEvent) -> Disposition {
        let started = Instant::now();
        let switch_hit = st.switch_key.feed(event);
        let home_hit = st
            .return_home_key
            .as_mut()
            .is_some_and(|matcher| matcher.feed(event));
        if let InputEvent::Keyboard(KeyboardEvent::KeyDown { modifiers, .. })
        | InputEvent::Keyboard(KeyboardEvent::KeyUp { modifiers, .. }) = event
        {
            st.modifiers = modifiers.clone();
        }

        if switch_hit || home_hit {
            // The chord's completing key never reaches any OS: its press
            // is swallowed here and its release by the held table.
            if let InputEvent::Keyboard(KeyboardEvent::KeyDown { key, .. }) = event {
                st.held.insert(Held::Key(key.clone()), Route::Swallow);
            }
            if st.debouncer.should_fire(started) {
                let target = if home_hit {
                    Some(ActiveScreen::Local)
                } else {
                    Self::next_screen(st)
                };
                if let Some(target) = target {
                    // A peer vanishing mid-cycle can't happen under this lock.
                    let _ = self.switch_locked(st, target, started);
                }
            }
            return Disposition::Swallow;
        }

        let route = self.route_of(st, event);
        match route {
            Route::Local => Disposition::Pass,
            Route::Swallow => Disposition::Swallow,
            Route::Remote(id) => {
                self.forward(st, &id, event.clone());
                Disposition::Swallow
            }
        }
    }

    /// Decides the route of `event`, recording presses and consuming
    /// releases in the held table.
    fn route_of(&self, st: &mut State, event: &InputEvent) -> Route {
        let current = || match &st.active {
            ActiveScreen::Local => Route::Local,
            ActiveScreen::Remote(id) => Route::Remote(id.clone()),
        };
        let shared = match event {
            InputEvent::Keyboard(_) => st.share_keyboard,
            InputEvent::Mouse(_) => st.share_mouse,
        };
        let fresh = if shared { current() } else { Route::Local };

        let (held, is_press) = match event {
            InputEvent::Keyboard(KeyboardEvent::KeyDown { key, .. }) => {
                (Some(Held::Key(key.clone())), true)
            }
            InputEvent::Keyboard(KeyboardEvent::KeyUp { key, .. }) => {
                (Some(Held::Key(key.clone())), false)
            }
            InputEvent::Mouse(MouseEvent::ButtonDown { button, .. }) => {
                (Some(Held::Button(*button)), true)
            }
            InputEvent::Mouse(MouseEvent::ButtonUp { button, .. }) => {
                (Some(Held::Button(*button)), false)
            }
            InputEvent::Mouse(_) => (None, false),
        };
        match held {
            None => fresh,
            Some(held) if is_press => {
                // A repeated press keeps the route of the original one.
                st.held.entry(held).or_insert(fresh).clone()
            }
            // A release follows its press; one with no known press belongs
            // to input that predates the capture, i.e. to this machine.
            Some(held) => st.held.remove(&held).unwrap_or(Route::Local),
        }
    }

    fn forward(&self, st: &mut State, id: &DeviceId, event: InputEvent) {
        let Some(os) = st.peers.iter().find(|p| &p.id == id).map(|p| p.os) else {
            st.events_dropped += 1;
            return;
        };
        let event = match event {
            InputEvent::Keyboard(_) => apply_remaps(event, os, &st.remap_rules),
            InputEvent::Mouse(MouseEvent::Move {
                dx,
                dy,
                timestamp_ms,
            }) => {
                let (dx, dy) = scale_pointer(st, dx, dy);
                if dx == 0 && dy == 0 {
                    return;
                }
                InputEvent::Mouse(MouseEvent::Move {
                    dx,
                    dy,
                    timestamp_ms,
                })
            }
            other => other,
        };
        if send(st, id, LinkOut::Input(event)) {
            st.events_forwarded += 1;
        } else {
            st.events_dropped += 1;
        }
    }
}

/// Queues `message` for `id`'s link task; `false` if the link is gone.
fn send(st: &State, id: &DeviceId, message: LinkOut) -> bool {
    st.peers
        .iter()
        .find(|p| &p.id == id)
        .is_some_and(|peer| peer.outbox.send(message).is_ok())
}

/// Where the pointer appears on a screen the user switches to by key.
fn entry_point(st: &State, id: &DeviceId) -> (i32, i32) {
    st.peers
        .iter()
        .find(|p| &p.id == id)
        .map(|p| (p.screen.width as i32 / 2, p.screen.height as i32 / 2))
        .unwrap_or((0, 0))
}

/// Applies the pointer-sensitivity scale, carrying the fraction that
/// rounding would otherwise lose so slow movement isn't eaten.
fn scale_pointer(st: &mut State, dx: i32, dy: i32) -> (i32, i32) {
    if (st.pointer_scale - 1.0).abs() < f32::EPSILON {
        return (dx, dy);
    }
    let sx = dx as f32 * st.pointer_scale + st.pointer_remainder.0;
    let sy = dy as f32 * st.pointer_scale + st.pointer_remainder.1;
    let (ox, oy) = (sx.trunc(), sy.trunc());
    st.pointer_remainder = (sx - ox, sy - oy);
    (ox as i32, oy as i32)
}

impl CaptureSink for InputRouter {
    fn on_event(&self, event: &InputEvent) -> Disposition {
        let mut st = self.lock();
        self.route_event(&mut st, event)
    }

    fn suppressing(&self) -> bool {
        self.suppressing.load(Ordering::SeqCst)
    }
}

/// A short, stable label for an event, for `flow::hop` log records.
pub fn event_kind(event: &InputEvent) -> &'static str {
    match event {
        InputEvent::Keyboard(KeyboardEvent::KeyDown { .. }) => "key_down",
        InputEvent::Keyboard(KeyboardEvent::KeyUp { .. }) => "key_up",
        InputEvent::Mouse(MouseEvent::Move { .. }) => "mouse_move",
        InputEvent::Mouse(MouseEvent::ButtonDown { .. }) => "mouse_button_down",
        InputEvent::Mouse(MouseEvent::ButtonUp { .. }) => "mouse_button_up",
        InputEvent::Mouse(MouseEvent::Scroll { .. }) => "mouse_scroll",
    }
}
