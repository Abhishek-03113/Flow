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

use flow_core::control::layout::{Edge, ScreenLayout};
use flow_core::control::ActiveScreen;
use flow_core::device::{DeviceId, HostOs, ScreenSize, LOCAL_DEVICE_ID};
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

const DEFAULT_SCREEN: ScreenSize = ScreenSize {
    width: 1920,
    height: 1080,
};

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
    layout: ScreenLayout,
    local_screen: ScreenSize,
    edge_switching: bool,
    edge_delay: Duration,
    /// The pointer on the active remote screen, in that screen's pixels,
    /// integrated from relative deltas.
    cursor: (i32, i32),
    /// Which edge the pointer is resting against, and since when.
    edge_rest: Option<(Edge, Instant)>,
    /// Where to put this machine's own pointer when edge switching
    /// returns home.
    local_warp: Option<mpsc::UnboundedSender<(i32, i32)>>,
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
                layout: ScreenLayout::local_only(DEFAULT_SCREEN),
                local_screen: DEFAULT_SCREEN,
                edge_switching: true,
                edge_delay: Duration::ZERO,
                cursor: (0, 0),
                edge_rest: None,
                local_warp: None,
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
        st.edge_switching = settings.edge_switching;
        st.edge_delay = Duration::from_millis(u64::from(settings.edge_switch_delay_ms));
    }

    pub fn set_layout(&self, layout: ScreenLayout) {
        self.lock().layout = layout;
    }

    pub fn set_local_screen(&self, screen: ScreenSize) {
        self.lock().local_screen = screen;
    }

    /// Where the router asks for this machine's own pointer to be moved
    /// when an edge switch brings control home.
    pub fn set_local_warp(&self, warp: mpsc::UnboundedSender<(i32, i32)>) {
        self.lock().local_warp = Some(warp);
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
            self.apply_switch(&mut st, ActiveScreen::Local, false, None);
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
            self.apply_switch(&mut st, ActiveScreen::Local, true, None);
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
        self.switch_locked(&mut st, target, started, None)
    }

    /// `entry` is where the pointer lands on `target` (its own pixels);
    /// `None` = the middle of a remote screen, no warp at home.
    fn switch_locked(
        &self,
        st: &mut State,
        target: ActiveScreen,
        started: Instant,
        entry: Option<(i32, i32)>,
    ) -> Result<(), SwitchError> {
        if let ActiveScreen::Remote(id) = &target {
            if !st.peers.iter().any(|p| &p.id == id) {
                return Err(SwitchError::NotConnected(id.clone()));
            }
        }
        if st.active == target {
            return Ok(());
        }
        self.apply_switch(st, target, true, entry);
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        if st.switch_latencies_ms.len() == MAX_LATENCY_SAMPLES {
            st.switch_latencies_ms.pop_front();
        }
        st.switch_latencies_ms.push_back(elapsed);
        Ok(())
    }

    /// The transition itself; `target` must be valid. `notify_old` is false
    /// when the previous remote is already gone.
    fn apply_switch(
        &self,
        st: &mut State,
        target: ActiveScreen,
        notify_old: bool,
        entry: Option<(i32, i32)>,
    ) {
        st.edge_rest = None;
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
                let (x, y) = entry.unwrap_or_else(|| centre_of(st, id));
                st.cursor = (x, y);
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
            ActiveScreen::Local => {
                self.suppressing.store(false, Ordering::SeqCst);
                if let (Some(point), Some(warp)) = (entry, &st.local_warp) {
                    let _ = warp.send(point);
                }
            }
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
                    let _ = self.switch_locked(st, target, started, None);
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

    /// Integrates a pointer delta on the remote screen `id`. When it
    /// pushes through an edge that has a neighbour — after resting there
    /// for the configured delay — switches to that neighbour and returns
    /// `true`.
    fn cross_edge(&self, st: &mut State, id: &DeviceId, dx: i32, dy: i32) -> bool {
        let Some(size) = st.peers.iter().find(|p| &p.id == id).map(|p| p.screen) else {
            return false;
        };
        let (w, h) = (size.width as i32, size.height as i32);
        let (vx, vy) = (st.cursor.0 + dx, st.cursor.1 + dy);
        st.cursor = (vx.clamp(0, w - 1), vy.clamp(0, h - 1));
        if !st.edge_switching {
            return false;
        }
        let (edge, along) = if vx < 0 {
            (Edge::Left, st.cursor.1)
        } else if vx >= w {
            (Edge::Right, st.cursor.1)
        } else if vy < 0 {
            (Edge::Top, st.cursor.0)
        } else if vy >= h {
            (Edge::Bottom, st.cursor.0)
        } else {
            st.edge_rest = None;
            return false;
        };
        let Some(crossing) = st.layout.neighbour(id, edge, along) else {
            st.edge_rest = None;
            return false;
        };
        let target = if crossing.to.0 == LOCAL_DEVICE_ID {
            ActiveScreen::Local
        } else if st.peers.iter().any(|p| p.id == crossing.to) {
            ActiveScreen::Remote(crossing.to.clone())
        } else {
            st.edge_rest = None;
            return false;
        };
        if !edge_dwelled(st, edge) {
            return false;
        }
        let _ = self.switch_locked(st, target, Instant::now(), Some((crossing.x, crossing.y)));
        true
    }

    /// Where this machine's own pointer is, while the local screen is
    /// active: pressing against an edge that has a connected neighbour
    /// switches to it, entering at the mirrored position. Fed by the
    /// engine from the backend's cursor position.
    pub fn local_cursor(&self, x: i32, y: i32) {
        let mut st = self.lock();
        if !st.edge_switching || st.active != ActiveScreen::Local {
            st.edge_rest = None;
            return;
        }
        let (w, h) = (st.local_screen.width as i32, st.local_screen.height as i32);
        let (edge, along) = if x <= 0 {
            (Edge::Left, y)
        } else if x >= w - 1 {
            (Edge::Right, y)
        } else if y <= 0 {
            (Edge::Top, x)
        } else if y >= h - 1 {
            (Edge::Bottom, x)
        } else {
            st.edge_rest = None;
            return;
        };
        let local = DeviceId(LOCAL_DEVICE_ID.to_string());
        let crossing = st
            .layout
            .neighbour(&local, edge, along)
            .filter(|c| st.peers.iter().any(|p| p.id == c.to));
        let Some(crossing) = crossing else {
            st.edge_rest = None;
            return;
        };
        if !edge_dwelled(&mut st, edge) {
            return;
        }
        let target = ActiveScreen::Remote(crossing.to);
        let _ = self.switch_locked(
            &mut st,
            target,
            Instant::now(),
            Some((crossing.x, crossing.y)),
        );
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
                if self.cross_edge(st, id, dx, dy) {
                    // The move that crossed the edge belongs to neither screen.
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

/// Whether the pointer has rested against `edge` for the configured delay
/// (starting the clock on first contact).
fn edge_dwelled(st: &mut State, edge: Edge) -> bool {
    let now = Instant::now();
    let since = match st.edge_rest {
        Some((resting, since)) if resting == edge => since,
        _ => {
            st.edge_rest = Some((edge, now));
            now
        }
    };
    now.duration_since(since) >= st.edge_delay
}

/// Queues `message` for `id`'s link task; `false` if the link is gone.
fn send(st: &State, id: &DeviceId, message: LinkOut) -> bool {
    st.peers
        .iter()
        .find(|p| &p.id == id)
        .is_some_and(|peer| peer.outbox.send(message).is_ok())
}

/// Where the pointer appears on a screen the user switches to by key.
fn centre_of(st: &State, id: &DeviceId) -> (i32, i32) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use flow_core::switch_key::presets;

    fn key(name: &str, down: bool) -> InputEvent {
        InputEvent::Keyboard(if down {
            KeyboardEvent::KeyDown {
                key: name.to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            }
        } else {
            KeyboardEvent::KeyUp {
                key: name.to_string(),
                modifiers: vec![],
                timestamp_ms: 0,
            }
        })
    }

    fn button(down: bool) -> InputEvent {
        InputEvent::Mouse(if down {
            MouseEvent::ButtonDown {
                button: MouseButton::Left,
                timestamp_ms: 0,
            }
        } else {
            MouseEvent::ButtonUp {
                button: MouseButton::Left,
                timestamp_ms: 0,
            }
        })
    }

    fn motion(dx: i32, dy: i32) -> InputEvent {
        InputEvent::Mouse(MouseEvent::Move {
            dx,
            dy,
            timestamp_ms: 0,
        })
    }

    fn router() -> InputRouter {
        let router = InputRouter::new(&FlowSettings::defaults(), Vec::new());
        router.set_debounce_window(Duration::ZERO);
        router
    }

    fn id(name: &str) -> DeviceId {
        DeviceId(name.to_string())
    }

    fn connect(router: &InputRouter, name: &str) -> mpsc::UnboundedReceiver<LinkOut> {
        connect_os(router, name, HostOs::Macos)
    }

    fn connect_os(
        router: &InputRouter,
        name: &str,
        os: HostOs,
    ) -> mpsc::UnboundedReceiver<LinkOut> {
        let (outbox, inbox) = mpsc::unbounded_channel();
        router.peer_connected(PeerSlot {
            id: id(name),
            os,
            screen: ScreenSize {
                width: 1000,
                height: 600,
            },
            outbox,
        });
        inbox
    }

    fn drain(inbox: &mut mpsc::UnboundedReceiver<LinkOut>) -> Vec<LinkOut> {
        std::iter::from_fn(|| inbox.try_recv().ok()).collect()
    }

    fn inputs(sent: &[LinkOut]) -> Vec<&InputEvent> {
        sent.iter()
            .filter_map(|m| match m {
                LinkOut::Input(e) => Some(e),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_switch_key_cycles_local_to_each_peer_and_back() {
        let r = router();
        let mut d1 = connect(&r, "d1");
        let mut d2 = connect(&r, "d2");

        assert_eq!(r.on_event(&key("SCROLLLOCK", true)), Disposition::Swallow);
        assert_eq!(r.active(), ActiveScreen::Remote(id("d1")));
        assert!(r.suppressing());
        assert!(matches!(drain(&mut d1)[..], [LinkOut::Enter { .. }]));
        r.on_event(&key("SCROLLLOCK", false));

        r.on_event(&key("SCROLLLOCK", true));
        assert_eq!(r.active(), ActiveScreen::Remote(id("d2")));
        assert!(r.suppressing(), "stays suppressed peer to peer");
        assert_eq!(drain(&mut d1), vec![LinkOut::Leave]);
        assert!(matches!(drain(&mut d2)[..], [LinkOut::Enter { .. }]));
        r.on_event(&key("SCROLLLOCK", false));

        r.on_event(&key("SCROLLLOCK", true));
        assert_eq!(r.active(), ActiveScreen::Local);
        assert!(!r.suppressing());
        assert_eq!(drain(&mut d2), vec![LinkOut::Leave]);
    }

    #[test]
    fn enter_is_queued_before_any_forwarded_input() {
        let r = router();
        let mut d1 = connect(&r, "d1");
        r.switch_to(ActiveScreen::Remote(id("d1"))).unwrap();
        assert_eq!(r.on_event(&key("A", true)), Disposition::Swallow);
        let sent = drain(&mut d1);
        assert!(matches!(sent[0], LinkOut::Enter { x: 500, y: 300, .. }));
        assert_eq!(inputs(&sent), vec![&key("A", true)]);
    }

    #[test]
    fn local_input_passes_and_is_never_forwarded() {
        let r = router();
        let mut d1 = connect(&r, "d1");
        assert_eq!(r.on_event(&key("A", true)), Disposition::Pass);
        assert_eq!(r.on_event(&motion(3, 3)), Disposition::Pass);
        assert!(drain(&mut d1).is_empty());
    }

    #[test]
    fn returning_home_always_works_while_input_is_suppressed() {
        let r = router();
        let mut d1 = connect(&r, "d1");
        r.switch_to(ActiveScreen::Remote(id("d1"))).unwrap();
        // The same capture still sees the hotkey although local apps don't.
        assert_eq!(r.on_event(&key("SCROLLLOCK", true)), Disposition::Swallow);
        assert_eq!(r.active(), ActiveScreen::Local);
        assert!(!r.suppressing());
        assert_eq!(drain(&mut d1).last(), Some(&LinkOut::Leave));
    }

    #[test]
    fn the_return_home_key_jumps_straight_to_local_from_any_peer() {
        let mut settings = FlowSettings::defaults();
        settings.return_home_key = Some(presets()[2].clone()); // F13
        let r = InputRouter::new(&settings, Vec::new());
        r.set_debounce_window(Duration::ZERO);
        let _d1 = connect(&r, "d1");
        let mut d2 = connect(&r, "d2");
        r.switch_to(ActiveScreen::Remote(id("d2"))).unwrap();

        assert_eq!(r.on_event(&key("F13", true)), Disposition::Swallow);
        assert_eq!(r.active(), ActiveScreen::Local);
        assert_eq!(drain(&mut d2).last(), Some(&LinkOut::Leave));
    }

    #[test]
    fn the_hotkey_chord_is_consumed_down_and_up_and_never_forwarded() {
        let r = router();
        let mut d1 = connect(&r, "d1");
        assert_eq!(r.on_event(&key("SCROLLLOCK", true)), Disposition::Swallow);
        assert_eq!(r.on_event(&key("SCROLLLOCK", false)), Disposition::Swallow);
        assert!(inputs(&drain(&mut d1)).is_empty());

        // And on the way back, while remote.
        assert_eq!(r.on_event(&key("SCROLLLOCK", true)), Disposition::Swallow);
        assert_eq!(r.on_event(&key("SCROLLLOCK", false)), Disposition::Swallow);
        assert!(inputs(&drain(&mut d1)).is_empty());
    }

    #[test]
    fn a_key_held_locally_across_a_switch_is_released_locally_not_forwarded() {
        let r = router();
        let mut d1 = connect(&r, "d1");
        assert_eq!(r.on_event(&key("A", true)), Disposition::Pass);
        r.switch_to(ActiveScreen::Remote(id("d1"))).unwrap();

        assert_eq!(r.on_event(&key("A", false)), Disposition::Pass);
        assert!(inputs(&drain(&mut d1)).is_empty());
    }

    #[test]
    fn a_key_held_remotely_across_a_switch_home_is_swallowed_and_the_remote_left() {
        let r = router();
        let mut d1 = connect(&r, "d1");
        r.switch_to(ActiveScreen::Remote(id("d1"))).unwrap();
        assert_eq!(r.on_event(&key("A", true)), Disposition::Swallow);
        r.switch_to(ActiveScreen::Local).unwrap();

        // Local apps never saw the press, so they must not see the release,
        // and the remote already released everything on Leave.
        assert_eq!(r.on_event(&key("A", false)), Disposition::Swallow);
        let sent = drain(&mut d1);
        assert_eq!(inputs(&sent), vec![&key("A", true)]);
        assert_eq!(sent.last(), Some(&LinkOut::Leave));
    }

    #[test]
    fn mouse_buttons_follow_the_same_up_follows_down_rule() {
        let r = router();
        let mut d1 = connect(&r, "d1");
        // Pressed locally, released while remote.
        assert_eq!(r.on_event(&button(true)), Disposition::Pass);
        r.switch_to(ActiveScreen::Remote(id("d1"))).unwrap();
        assert_eq!(r.on_event(&button(false)), Disposition::Pass);
        // Pressed remotely, released after returning home.
        assert_eq!(r.on_event(&button(true)), Disposition::Swallow);
        r.switch_to(ActiveScreen::Local).unwrap();
        assert_eq!(r.on_event(&button(false)), Disposition::Swallow);

        assert_eq!(inputs(&drain(&mut d1)), vec![&button(true)]);
    }

    #[test]
    fn a_release_with_no_known_press_belongs_to_this_machine() {
        let r = router();
        let mut d1 = connect(&r, "d1");
        r.switch_to(ActiveScreen::Remote(id("d1"))).unwrap();
        assert_eq!(r.on_event(&key("Z", false)), Disposition::Pass);
        assert!(inputs(&drain(&mut d1)).is_empty());
    }

    #[test]
    fn losing_the_active_link_returns_home_and_unsuppresses() {
        let r = router();
        let _d1 = connect(&r, "d1");
        r.switch_to(ActiveScreen::Remote(id("d1"))).unwrap();
        assert_eq!(r.on_event(&key("A", true)), Disposition::Swallow);

        r.peer_disconnected(&id("d1"));

        assert_eq!(r.active(), ActiveScreen::Local);
        assert!(!r.suppressing());
        assert!(r.peer_ids().is_empty());
        assert_eq!(r.on_event(&motion(1, 1)), Disposition::Pass);
        // The orphaned release is still swallowed, never delivered locally.
        assert_eq!(r.on_event(&key("A", false)), Disposition::Swallow);
    }

    #[test]
    fn losing_an_inactive_link_changes_nothing_else() {
        let r = router();
        let _d1 = connect(&r, "d1");
        let _d2 = connect(&r, "d2");
        r.switch_to(ActiveScreen::Remote(id("d2"))).unwrap();
        r.peer_disconnected(&id("d1"));
        assert_eq!(r.active(), ActiveScreen::Remote(id("d2")));
        assert!(r.suppressing());
    }

    #[test]
    fn a_release_request_from_the_active_peer_goes_home() {
        let r = router();
        let mut d1 = connect(&r, "d1");
        r.switch_to(ActiveScreen::Remote(id("d1"))).unwrap();
        r.release_requested(&id("d1"));
        assert_eq!(r.active(), ActiveScreen::Local);
        assert_eq!(drain(&mut d1).last(), Some(&LinkOut::Leave));

        // From a peer that isn't active it is ignored.
        r.release_requested(&id("d1"));
        assert_eq!(r.active(), ActiveScreen::Local);
    }

    #[test]
    fn switching_to_a_peer_without_a_link_is_an_error_and_changes_nothing() {
        let r = router();
        assert_eq!(
            r.switch_to(ActiveScreen::Remote(id("ghost"))),
            Err(SwitchError::NotConnected(id("ghost")))
        );
        assert_eq!(r.active(), ActiveScreen::Local);
        assert!(!r.suppressing());
    }

    #[test]
    fn the_hotkey_with_no_peers_does_nothing_but_is_still_consumed() {
        let r = router();
        assert_eq!(r.on_event(&key("SCROLLLOCK", true)), Disposition::Swallow);
        assert_eq!(r.active(), ActiveScreen::Local);
    }

    #[test]
    fn repeated_matches_inside_the_debounce_window_switch_once() {
        let r = InputRouter::new(&FlowSettings::defaults(), Vec::new());
        let _d1 = connect(&r, "d1");
        r.on_event(&key("SCROLLLOCK", true));
        r.on_event(&key("SCROLLLOCK", false));
        r.on_event(&key("SCROLLLOCK", true));
        assert_eq!(r.active(), ActiveScreen::Remote(id("d1")));
    }

    #[test]
    fn keyboard_stays_local_when_keyboard_sharing_is_off() {
        let mut settings = FlowSettings::defaults();
        settings.share_keyboard = false;
        let r = InputRouter::new(&settings, Vec::new());
        let mut d1 = connect(&r, "d1");
        r.switch_to(ActiveScreen::Remote(id("d1"))).unwrap();

        assert_eq!(r.on_event(&key("A", true)), Disposition::Pass);
        assert_eq!(r.on_event(&motion(1, 1)), Disposition::Swallow);
        assert_eq!(inputs(&drain(&mut d1)), vec![&motion(1, 1)]);
    }

    #[test]
    fn the_mouse_stays_local_when_mouse_sharing_is_off() {
        let mut settings = FlowSettings::defaults();
        settings.share_mouse = false;
        let r = InputRouter::new(&settings, Vec::new());
        let mut d1 = connect(&r, "d1");
        r.switch_to(ActiveScreen::Remote(id("d1"))).unwrap();

        assert_eq!(r.on_event(&motion(1, 1)), Disposition::Pass);
        assert_eq!(r.on_event(&key("A", true)), Disposition::Swallow);
        assert_eq!(inputs(&drain(&mut d1)), vec![&key("A", true)]);
    }

    #[test]
    fn pointer_sensitivity_scales_deltas_without_losing_the_fraction() {
        let mut settings = FlowSettings::defaults();
        settings.pointer_sensitivity = PointerSensitivity::Low;
        let r = InputRouter::new(&settings, Vec::new());
        let mut d1 = connect(&r, "d1");
        r.switch_to(ActiveScreen::Remote(id("d1"))).unwrap();

        for _ in 0..10 {
            r.on_event(&motion(1, 0));
        }
        let total: i32 = inputs(&drain(&mut d1))
            .into_iter()
            .map(|e| match e {
                InputEvent::Mouse(MouseEvent::Move { dx, .. }) => *dx,
                _ => 0,
            })
            .sum();
        assert_eq!(total, 6, "10 x 0.6 reaches the remote as 6 in total");
    }

    #[test]
    fn remaps_apply_for_the_target_os_only() {
        let rules = vec![RemapRule {
            from: "CAPS_LOCK".to_string(),
            to: "ESCAPE".to_string(),
            when_target_os: Some(HostOs::Macos),
        }];
        let r = InputRouter::new(&FlowSettings::defaults(), rules);
        let mut mac = connect_os(&r, "mac", HostOs::Macos);
        let mut win = connect_os(&r, "win", HostOs::Windows);

        r.switch_to(ActiveScreen::Remote(id("mac"))).unwrap();
        r.on_event(&key("CAPS_LOCK", true));
        assert_eq!(inputs(&drain(&mut mac)), vec![&key("ESCAPE", true)]);

        r.on_event(&key("CAPS_LOCK", false));
        r.switch_to(ActiveScreen::Remote(id("win"))).unwrap();
        r.on_event(&key("CAPS_LOCK", true));
        assert_eq!(inputs(&drain(&mut win)), vec![&key("CAPS_LOCK", true)]);
    }

    #[test]
    fn a_rebind_takes_effect_for_the_next_event() {
        let r = router();
        let _d1 = connect(&r, "d1");
        let mut settings = FlowSettings::defaults();
        settings.switch_key = presets()[2].clone(); // F13
        r.apply_settings(&settings);

        assert_eq!(r.on_event(&key("SCROLLLOCK", true)), Disposition::Pass);
        assert_eq!(r.on_event(&key("F13", true)), Disposition::Swallow);
        assert_eq!(r.active(), ActiveScreen::Remote(id("d1")));
    }

    #[test]
    fn switches_and_forwarded_events_are_counted() {
        let r = router();
        let _d1 = connect(&r, "d1");
        r.switch_to(ActiveScreen::Remote(id("d1"))).unwrap();
        r.on_event(&key("A", true));
        r.on_event(&motion(2, 2));
        let stats = r.stats();
        assert_eq!(stats.events_forwarded, 2);
        assert_eq!(stats.switch_latencies_ms.len(), 1);
    }

    // ---- screen layout and edge switching ----

    use flow_core::control::layout::Placement;

    fn place(name: &str, x: i32, y: i32, width: u32, height: u32) -> Placement {
        Placement {
            device_id: id(name),
            x,
            y,
            width,
            height,
        }
    }

    /// d1 (1920x1080) | p (1000x600) | q (800x600), side by side at y=0.
    fn edge_router() -> InputRouter {
        let r = router();
        r.set_local_screen(ScreenSize {
            width: 1920,
            height: 1080,
        });
        r.set_layout(ScreenLayout {
            screens: vec![
                place("d1", 0, 0, 1920, 1080),
                place("p", 1920, 0, 1000, 600),
                place("q", 2920, 0, 800, 600),
            ],
        });
        r
    }

    #[test]
    fn pressing_the_local_right_edge_enters_the_neighbour_at_the_mirrored_height() {
        let r = edge_router();
        let mut p = connect(&r, "p");
        r.local_cursor(960, 300);
        assert_eq!(r.active(), ActiveScreen::Local);

        r.local_cursor(1919, 300);

        assert_eq!(r.active(), ActiveScreen::Remote(id("p")));
        assert!(r.suppressing());
        assert!(matches!(
            drain(&mut p)[..],
            [LinkOut::Enter { x: 1, y: 300, .. }]
        ));
    }

    #[test]
    fn an_edge_with_no_neighbour_at_that_height_does_not_switch() {
        let r = edge_router();
        let _p = connect(&r, "p");
        r.local_cursor(1919, 800); // below the 600-high neighbour
        r.local_cursor(0, 300); // nothing to the left
        assert_eq!(r.active(), ActiveScreen::Local);
    }

    #[test]
    fn a_neighbour_that_is_not_connected_is_not_entered() {
        let r = edge_router();
        r.local_cursor(1919, 300);
        assert_eq!(r.active(), ActiveScreen::Local);
    }

    #[test]
    fn edge_switching_can_be_turned_off() {
        let r = edge_router();
        let mut settings = FlowSettings::defaults();
        settings.edge_switching = false;
        r.apply_settings(&settings);
        let mut p = connect(&r, "p");
        r.local_cursor(1919, 300);
        assert_eq!(r.active(), ActiveScreen::Local);

        r.switch_to(ActiveScreen::Remote(id("p"))).unwrap();
        drain(&mut p);
        r.on_event(&motion(-2000, 0));
        assert_eq!(r.active(), ActiveScreen::Remote(id("p")));
    }

    #[test]
    fn the_pointer_must_rest_at_the_edge_for_the_configured_delay() {
        let r = edge_router();
        let mut settings = FlowSettings::defaults();
        settings.edge_switch_delay_ms = 40;
        r.apply_settings(&settings);
        let _p = connect(&r, "p");

        r.local_cursor(1919, 300);
        assert_eq!(r.active(), ActiveScreen::Local);
        std::thread::sleep(Duration::from_millis(60));
        r.local_cursor(1919, 300);
        assert_eq!(r.active(), ActiveScreen::Remote(id("p")));
    }

    #[test]
    fn leaving_the_edge_resets_the_dwell_clock() {
        let r = edge_router();
        let mut settings = FlowSettings::defaults();
        settings.edge_switch_delay_ms = 40;
        r.apply_settings(&settings);
        let _p = connect(&r, "p");

        r.local_cursor(1919, 300);
        std::thread::sleep(Duration::from_millis(60));
        r.local_cursor(1000, 300); // moved away
        r.local_cursor(1919, 300); // fresh contact
        assert_eq!(r.active(), ActiveScreen::Local);
    }

    #[test]
    fn pushing_back_through_the_left_edge_returns_home_and_warps_the_local_pointer() {
        let r = edge_router();
        let (warp_tx, mut warps) = mpsc::unbounded_channel();
        r.set_local_warp(warp_tx);
        let mut p = connect(&r, "p");
        r.local_cursor(1919, 300);
        drain(&mut p);

        // Entered one pixel inside the left edge; two steps left cross back.
        assert_eq!(r.on_event(&motion(-3, 0)), Disposition::Swallow);

        assert_eq!(r.active(), ActiveScreen::Local);
        assert!(!r.suppressing());
        assert_eq!(
            drain(&mut p),
            vec![LinkOut::Leave],
            "the crossing move is not forwarded"
        );
        assert_eq!(
            warps.try_recv().ok(),
            Some((1918, 300)),
            "home, one pixel inside the edge so the pointer does not bounce back"
        );
    }

    #[test]
    fn moves_inside_the_remote_screen_are_forwarded_and_integrated() {
        let r = edge_router();
        let mut p = connect(&r, "p");
        r.switch_to(ActiveScreen::Remote(id("p"))).unwrap(); // enters at 500,300
        r.on_event(&motion(100, 50));
        r.on_event(&motion(10, 0));
        assert_eq!(r.active(), ActiveScreen::Remote(id("p")));
        assert_eq!(inputs(&drain(&mut p)).len(), 2);
    }

    #[test]
    fn crossing_between_two_remote_screens_leaves_one_and_enters_the_other() {
        let r = edge_router();
        let mut p = connect(&r, "p");
        let mut q = connect(&r, "q");
        r.switch_to(ActiveScreen::Remote(id("p"))).unwrap();
        drain(&mut p);

        r.on_event(&motion(600, 0)); // 500 -> 1100 > width 1000: right edge

        assert_eq!(r.active(), ActiveScreen::Remote(id("q")));
        assert!(r.suppressing());
        assert_eq!(drain(&mut p), vec![LinkOut::Leave]);
        assert!(matches!(
            drain(&mut q)[..],
            [LinkOut::Enter { x: 1, y: 300, .. }]
        ));
    }

    #[test]
    fn an_outer_edge_with_no_neighbour_just_keeps_the_pointer_on_screen() {
        let r = edge_router();
        let mut q = connect(&r, "q");
        r.switch_to(ActiveScreen::Remote(id("q"))).unwrap();
        drain(&mut q);
        assert_eq!(r.on_event(&motion(5000, 0)), Disposition::Swallow);
        assert_eq!(r.active(), ActiveScreen::Remote(id("q")));
        assert_eq!(inputs(&drain(&mut q)).len(), 1);
    }
}
