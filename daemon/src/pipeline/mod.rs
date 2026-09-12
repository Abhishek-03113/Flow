//! The end-to-end input streaming pipeline (`daemon/todos.json` G8):
//! capture (E1) -> switch-aware gate (only while the *peer* is the
//! active device, per F2/F3's switch state — see
//! [`is_peer_receiving_input`]) -> `Channel::send` on the sending side;
//! `Channel::recv` -> injector (E2) on the receiving side. Coded entirely against `flow_core::channel::Channel` and
//! `flow_core::input::{InputCapture, InputInjector}` — never a concrete
//! medium or platform type — so the gating logic here is exactly what
//! this module's own tests exercise without real hardware or a real
//! network (a real, loopback-connected `TcpChannel` stands in for "any
//! `Channel`", the same substitution `channel::negotiate`/`::handshake`'s
//! own tests already make).
//!
//! `vision.md`'s North Star ("just press a key and continue working")
//! is this pipeline: it's the first place capture, a `Channel`, and
//! injection are wired into one continuous loop.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use flow_core::channel::{Channel, ChannelMessage};
use flow_core::device::{Device, DeviceId, DeviceState};
use flow_core::input::InputInjector;
use flow_core::protocol::{InputEvent, InputRole, KeyboardEvent, MouseButton, MouseEvent};
use tokio::sync::{mpsc, watch};

use crate::service::LOCAL_DEVICE_ID;

/// Whether `peer_id` is the device currently receiving input — i.e.
/// whether input captured here should be forwarded to it.
///
/// **`Active` means the device input is being sent *to*, not the one
/// it's captured on.** `docs/product/vision.md` §22 states it directly
/// ("Only the active device should receive input"), and the tray UI
/// agrees: it lists the active device under a "Using" heading, meaning
/// the machine you're currently driving. So the machine with the
/// physical keyboard forwards only while some *other* device is active,
/// and keeps its own input to itself while it is the active one.
///
/// Gating on the specific peer this connection serves — rather than
/// simply "the local device isn't active" — is what keeps a third
/// device out of it: with A, B and C paired and B active, A must send to
/// B alone, not blast every captured event down C's connection too.
fn is_peer_receiving_input(devices: &[Device], peer_id: &DeviceId) -> bool {
    devices
        .iter()
        .find(|device| &device.id == peer_id)
        .is_some_and(|device| device.state == DeviceState::Active)
}

/// The sending side: forwards every captured event onto `channel` as a
/// `ChannelMessage::Input`, tagged with a per-connection sequence number
/// (`daemon/todos.json` H4, revised — see `ChannelMessage::Input`'s own
/// doc comment for why this replaced a timestamp-based check), but only
/// while `peer_id` is the active (receiving) device per `devices` — an
/// event captured while this machine is the active one is silently
/// dropped, not queued for later, and does *not* consume a sequence
/// number. Returns once `capture_events` closes (capture stopped) or
/// `channel.send` fails (peer gone).
///
/// One-directional, so only useful where this side is known to be the
/// sender for the connection's whole lifetime (`daemon/examples/`, and
/// this module's own tests). The daemon itself runs
/// [`run_paired_connection`] instead, since either end of a real peer
/// connection can become the active device at any point.
pub async fn send_while_active(
    mut capture_events: mpsc::UnboundedReceiver<InputEvent>,
    mut devices: watch::Receiver<Vec<Device>>,
    mut channel: Box<dyn Channel>,
    peer_id: DeviceId,
) {
    let mut sequence: u64 = 0;
    loop {
        tokio::select! {
            event = capture_events.recv() => {
                let Some(event) = event else { break; };
                if is_peer_receiving_input(&devices.borrow_and_update(), &peer_id) {
                    sequence += 1;
                    if channel.send(ChannelMessage::Input { sequence, event }).await.is_err() {
                        break;
                    }
                }
            }
            changed = devices.changed() => {
                if changed.is_err() {
                    break;
                }
            }
        }
    }
}

/// The receiving side: injects every `ChannelMessage::Input` that
/// arrives over `channel`. Anything else on the same connection
/// (`Pairing`/`Heartbeat` traffic sharing it once G7's handshake and
/// this pipeline run concurrently) is ignored rather than treated as an
/// error. A single failed `inject` (e.g. a transient OS-level rejection)
/// doesn't end the loop — only the `Channel` closing does.
///
/// Replay protection (`daemon/todos.json` H4): each message's sender-
/// assigned `sequence` must strictly increase from the last *accepted*
/// message's — anything arriving with an equal or lower sequence is a
/// duplicate or replayed frame and is dropped, not injected. Deliberately
/// not derived from `event.timestamp_ms()`: two legitimate high-frequency
/// events (consecutive mouse-move deltas, say) can land on the same
/// millisecond under a coarse OS clock, which a timestamp-based check
/// can't distinguish from an actual replay without either wrongly
/// dropping real input or wrongly accepting a replay.
///
/// Stuck-input safety (daemon review gap #18): if the connection drops
/// between a `KeyDown`/`ButtonDown` this loop already injected and its
/// matching `KeyUp`/`ButtonUp`, the remote OS is left believing that
/// key/button is held forever — there's no third party to tell it
/// otherwise once the `Channel` that would have carried the release is
/// gone. `HeldInputTracker` exists specifically to make that release
/// happen anyway, synthesized locally, the moment this loop ends for any
/// reason.
pub async fn receive_and_inject<I>(mut channel: Box<dyn Channel>, mut injector: I)
where
    I: InputInjector,
    I::Error: std::fmt::Debug,
{
    let mut last_sequence: Option<u64> = None;
    let mut held = HeldInputTracker::default();
    loop {
        match channel.recv().await {
            Ok(ChannelMessage::Input { sequence, event }) => {
                if last_sequence.is_some_and(|last| sequence <= last) {
                    continue;
                }
                last_sequence = Some(sequence);
                match injector.inject(&event) {
                    Ok(()) => held.observe(&event),
                    Err(err) => crate::logging::product::error("inject input", &err),
                }
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    held.release_all(&mut injector);
}

/// Tracks which keys/mouse buttons this side has injected a `KeyDown`/
/// `ButtonDown` for with no matching `KeyUp`/`ButtonUp` seen yet, so
/// [`receive_and_inject`] can synthesize the release itself once the
/// `Channel` that would have carried a real one is gone — the mitigation
/// `daemon/todos.json`'s review calls a "hard invariant": a dropped
/// connection must never leave the remote OS believing input is
/// permanently held.
#[derive(Default)]
struct HeldInputTracker {
    keys: HashSet<String>,
    buttons: HashSet<MouseButton>,
}

impl HeldInputTracker {
    /// Updates held state from an event this side just successfully
    /// injected. Only ever called on a successful `inject` — an event the
    /// OS never actually saw shouldn't register as newly held.
    fn observe(&mut self, event: &InputEvent) {
        match event {
            InputEvent::Keyboard(KeyboardEvent::KeyDown { key, .. }) => {
                self.keys.insert(key.clone());
            }
            InputEvent::Keyboard(KeyboardEvent::KeyUp { key, .. }) => {
                self.keys.remove(key);
            }
            InputEvent::Mouse(MouseEvent::ButtonDown { button, .. }) => {
                self.buttons.insert(*button);
            }
            InputEvent::Mouse(MouseEvent::ButtonUp { button, .. }) => {
                self.buttons.remove(button);
            }
            InputEvent::Mouse(MouseEvent::Move { .. })
            | InputEvent::Mouse(MouseEvent::Scroll { .. }) => {}
        }
    }

    /// Synthetic `KeyUp`/`ButtonUp` events for everything currently tracked
    /// as held, clearing the tracker in the process. Shared by
    /// [`Self::release_all`] (inject the releases locally on disconnect)
    /// and the send side of [`run_paired_connection`] (forward the
    /// releases to the peer when this machine hands the ownership baton
    /// away mid-hold, so the peer isn't left with a stuck key).
    fn drain_releases(&mut self) -> Vec<InputEvent> {
        let timestamp_ms = now_ms();
        let mut releases = Vec::with_capacity(self.keys.len() + self.buttons.len());
        for key in self.keys.drain() {
            releases.push(InputEvent::Keyboard(KeyboardEvent::KeyUp {
                key,
                modifiers: Vec::new(),
                timestamp_ms,
            }));
        }
        for button in self.buttons.drain() {
            releases.push(InputEvent::Mouse(MouseEvent::ButtonUp {
                button,
                timestamp_ms,
            }));
        }
        releases
    }

    /// Synthesizes and injects a `KeyUp`/`ButtonUp` for everything still
    /// tracked as held, then clears. A failure releasing one held
    /// key/button doesn't stop the rest from being attempted — a partial
    /// release is still strictly better than releasing none.
    fn release_all<I>(&mut self, injector: &mut I)
    where
        I: InputInjector,
        I::Error: std::fmt::Debug,
    {
        for event in self.drain_releases() {
            if let Err(err) = injector.inject(&event) {
                crate::logging::product::error("release held input on disconnect", &err);
            }
        }
    }
}

/// The full-duplex counterpart to [`send_while_active`]/[`receive_and_inject`]
/// for a single already-authenticated connection to the paired peer
/// `peer_id`, where either side may become the active device at
/// different points over that connection's lifetime — so this daemon
/// must be able to both send (while the peer is active) and
/// receive-and-inject (while this machine is) over the *same*
/// connection, not two separate ones.
///
/// `Channel::send`/`::recv` both take `&mut self`, and nothing in this
/// codebase splits a `Channel` into independent read/write halves (see
/// `channel::noise::NoiseChannel`'s single shared `TransportState`, used
/// for both directions — splitting it safely would need real redesign,
/// not just here). So rather than run `send_while_active` and
/// `receive_and_inject` concurrently on two tasks sharing one channel,
/// this single task interleaves both directions itself with
/// `tokio::select!` — the same technique `send_while_active` already
/// uses to race its `capture_events`/`devices` inputs against each
/// other, just extended to a third branch reading off `channel` too.
///
/// `suppress_local` is called whenever this side's [`InputRole`] changes,
/// with `true` while input is being forwarded away from this machine
/// (i.e. this side is [`InputRole::Primary`]). Without it, capture is
/// purely passive on every platform and a forwarded keystroke would land
/// on *both* machines — see
/// `flow_core::input::InputCapture::set_suppress_local`. It's passed as a
/// closure rather than an `InputCapture` handle because the capture
/// object lives on the caller's side of a thread boundary; the caller
/// decides how to reach it, and reports failures (a platform that can't
/// suppress) however it sees fit.
///
/// `on_peer_ownership` is called with this daemon's new [`InputRole`]
/// whenever the *peer* hands the ownership baton across
/// (`ChannelMessage::OwnershipChanged`) — the caller wires it to
/// `DaemonService::apply_peer_ownership` so the local device list follows
/// the peer's switch without a reconnect. A switch initiated *here*
/// (Scroll Lock on this machine, or an IPC `switch_active_device`) is
/// observed through `devices` instead and relayed to the peer over the
/// same connection; ownership is never inferred from both machines
/// independently detecting the switch key.
///
/// `ownership` ([`crate::ownership::OwnershipHandle`]) is this
/// connection's single source of truth for *this daemon's own role* and
/// the ownership generation counter, shared (not copied) with
/// `DaemonService` and `hotkey::runner::spawn_pipeline_switch_filter` —
/// updated here at every point `forwarding` changes (the opening
/// handshake, a local flip, and a received `OwnershipChanged`), and
/// gating a received live handoff against stale/duplicate delivery
/// before anything else in this function reacts to it.
///
/// **Not** reset to `Primary` when this connection ends ("Fix Flow V1
/// Ownership Synchronization" task): doing that unconditionally, on
/// *both* peers, independent of what either side's role actually was,
/// is exactly what produced split-brain ownership on a disconnect —
/// both sides would resolve to `Primary` the instant their shared
/// connection dropped, with nothing to reconcile them afterward. Role is
/// instead left exactly as it was; the next connection's opening
/// handshake (see the `resolve_ownership` call near the top of this
/// function) reconciles it deterministically, whether that next
/// connection is a reconnect to the same peer or this daemon's first
/// connection ever.
// `ownership` (an `OwnershipHandle`, cheap and Clone) is the 8th
// parameter, one over clippy's default limit of 7 — bundling the
// existing 7 into a struct to silence this is a larger, unrelated
// refactor of every call site (including the 8 in this module's own
// tests) for a lint threshold, not a real complexity problem here.
#[allow(clippy::too_many_arguments)]
pub async fn run_paired_connection<I, S, O>(
    mut channel: Box<dyn Channel>,
    mut capture_events: mpsc::UnboundedReceiver<InputEvent>,
    mut devices: watch::Receiver<Vec<Device>>,
    mut injector: I,
    peer_id: DeviceId,
    mut suppress_local: S,
    mut on_peer_ownership: O,
    ownership: crate::ownership::OwnershipHandle,
) where
    I: InputInjector,
    I::Error: std::fmt::Debug,
    S: FnMut(bool),
    O: FnMut(InputRole),
{
    let mut send_sequence: u64 = 0;
    let mut last_received_sequence: Option<u64> = None;
    // Held input this side has *injected* from the peer (released locally
    // on disconnect) and held input this side has *forwarded* to the peer
    // (released to the peer when this side hands the baton away mid-hold,
    // task §11).
    let mut held = HeldInputTracker::default();
    let mut sent_held = HeldInputTracker::default();

    // Ownership synchronization handshake ("Fix Flow V1 Ownership
    // Synchronization" task §5): before anything else touches forwarding,
    // suppression, or the devices list, both sides exchange their current
    // belief about who is Primary and independently resolve any
    // disagreement the same deterministic way (`resolve_ownership`). This
    // runs on *every* new connection — a reconnect after a disconnect, a
    // simultaneous connection race, or the very first connection this pair
    // ever makes — so it is the one mechanism that keeps two peers from
    // ever both landing on Primary, rather than each side just re-deriving
    // its own belief from its own local `devices` snapshot as before.
    let local_id = DeviceId(LOCAL_DEVICE_ID.to_string());
    let local_belief = local_belief(&ownership, &peer_id);
    let local_generation = ownership.generation_now();
    if channel
        .send(ChannelMessage::OwnershipChanged {
            primary_device_id: local_belief.clone(),
            generation: local_generation,
        })
        .await
        .is_err()
    {
        crate::hop_note!(
            stage = "send_failed",
            role = "owner",
            peer = %peer_id.0,
            "channel send failed during the opening ownership handshake; not starting the pipeline"
        );
        return;
    }
    let (peer_belief, peer_generation) = loop {
        match channel.recv().await {
            Ok(ChannelMessage::OwnershipChanged {
                primary_device_id,
                generation,
            }) => break (primary_device_id, generation),
            // A Heartbeat or other frame racing the handshake on the same
            // connection isn't part of it — left for the main loop below,
            // not treated as an error here.
            Ok(_) => continue,
            Err(_) => {
                crate::hop_note!(
                    stage = "recv_failed",
                    role = "owner",
                    peer = %peer_id.0,
                    "channel recv failed during the opening ownership handshake; not starting the pipeline"
                );
                return;
            }
        }
    };
    let (resolved_primary, resolved_generation) = resolve_ownership(
        &local_id,
        &local_belief,
        local_generation,
        &peer_id,
        &peer_belief,
        peer_generation,
    );
    let role_changed = ownership.reconcile(is_local_device(&resolved_primary), resolved_generation);
    // `forwarding` is the single source of truth for this side's role for
    // the rest of the loop — it is updated the instant an `OwnershipChanged`
    // arrives, so the send gate is never one `devices` tick behind during a
    // handoff.
    let mut forwarding = is_local_device(&resolved_primary);
    suppress_local(forwarding);
    on_peer_ownership(role_of(forwarding));
    crate::hop_note!(
        stage = "ownership_reconciled",
        role = "owner",
        peer = %peer_id.0,
        local_belief = %local_belief.0,
        local_generation = local_generation,
        peer_belief = %peer_belief.0,
        peer_generation = peer_generation,
        resolved_primary = %resolved_primary.0,
        resolved_generation = resolved_generation,
        role_changed = role_changed,
        forwarding = forwarding,
        "ownership reconciled at connection start; initial send-gate state set"
    );

    'conn: loop {
        tokio::select! {
            event = capture_events.recv() => {
                let Some(event) = event else { break 'conn; };
                crate::hop!(
                    stage = "send_gate",
                    role = "owner",
                    peer = %peer_id.0,
                    route = if forwarding { "remote" } else { "local" },
                    forwarded = forwarding,
                    // For the sending side, local input is suppressed
                    // exactly while it is being forwarded away.
                    suppressed = forwarding,
                    kind = event_kind(&event),
                    "captured event reached the send gate"
                );
                if forwarding {
                    send_sequence += 1;
                    let detail = describe_event(&event);
                    sent_held.observe(&event);
                    if channel.send(ChannelMessage::Input { sequence: send_sequence, event }).await.is_err() {
                        crate::hop_note!(
                            stage = "send_failed",
                            role = "owner",
                            peer = %peer_id.0,
                            seq = send_sequence,
                            "channel send failed; ending pipeline"
                        );
                        break 'conn;
                    }
                    crate::hop!(
                        stage = "frame_sent",
                        role = "owner",
                        peer = %peer_id.0,
                        seq = send_sequence,
                        "input frame sent to the active peer"
                    );
                    let snapshot = devices.borrow();
                    crate::logging::product::input(
                        &device_name(&snapshot, crate::service::LOCAL_DEVICE_ID),
                        &device_name(&snapshot, &peer_id.0),
                        &detail,
                    );
                }
            }
            changed = devices.changed() => {
                if changed.is_err() {
                    break 'conn;
                }
                let now_forwarding =
                    is_peer_receiving_input(&devices.borrow_and_update(), &peer_id);
                if now_forwarding == forwarding {
                    // Some other device field changed — role is unaffected.
                    continue;
                }
                // A switch initiated on *this* machine (Scroll Lock here,
                // or an IPC `switch_active_device`). Relay it to the peer
                // over the same connection so the two stay consistent
                // without a reconnect — and don't strand anything we were
                // mid-forward when we stop being Primary.
                let new_role = role_of(now_forwarding);
                if forwarding && !now_forwarding
                    && forward_releases(&mut channel, &mut send_sequence, sent_held.drain_releases()).await.is_err()
                {
                    break 'conn;
                }
                let generation = ownership.bump_generation();
                let primary_device_id = if now_forwarding {
                    local_id.clone()
                } else {
                    peer_id.clone()
                };
                if channel.send(ChannelMessage::OwnershipChanged { primary_device_id, generation }).await.is_err() {
                    crate::hop_note!(
                        stage = "send_failed",
                        role = "owner",
                        peer = %peer_id.0,
                        "channel send failed handing the ownership baton; ending pipeline"
                    );
                    break 'conn;
                }
                forwarding = now_forwarding;
                ownership.set_role(new_role);
                // Going Secondary -> Primary: release anything we were
                // injecting from the peer. The peer's own hand-off flush
                // covers this too, but reconciling both sides keeps a
                // missed frame from stranding a key (task §11).
                if now_forwarding {
                    held.release_all(&mut injector);
                }
                crate::hop_note!(
                    stage = "input_role_changed",
                    role = "owner",
                    peer = %peer_id.0,
                    trigger = "local",
                    new_role = ?new_role,
                    "local switch relayed to the peer over the live connection"
                );
                suppress_local(forwarding);
            }
            received = channel.recv() => {
                match received {
                    Ok(ChannelMessage::Input { sequence, event }) => {
                        if last_received_sequence.is_some_and(|last| sequence <= last) {
                            crate::hop!(
                                stage = "replay_drop",
                                role = "receiver",
                                peer = %peer_id.0,
                                seq = sequence,
                                "dropped a frame at or below the last accepted sequence"
                            );
                            continue;
                        }
                        last_received_sequence = Some(sequence);
                        crate::hop!(
                            stage = "frame_recv",
                            role = "receiver",
                            peer = %peer_id.0,
                            seq = sequence,
                            kind = event_kind(&event),
                            "input frame received from the peer"
                        );
                        match injector.inject(&event) {
                            Ok(()) => {
                                crate::hop!(
                                    stage = "injected",
                                    role = "receiver",
                                    peer = %peer_id.0,
                                    seq = sequence,
                                    "event injected into this machine"
                                );
                                let snapshot = devices.borrow();
                                crate::logging::product::input(
                                    &device_name(&snapshot, &peer_id.0),
                                    &device_name(&snapshot, crate::service::LOCAL_DEVICE_ID),
                                    &describe_event(&event),
                                );
                                drop(snapshot);
                                held.observe(&event)
                            }
                            Err(err) => crate::logging::product::error("inject input", &err),
                        }
                    }
                    Ok(ChannelMessage::OwnershipChanged { primary_device_id, generation }) => {
                        if primary_device_id != local_id && primary_device_id != peer_id {
                            crate::hop_note!(
                                stage = "ownership_rejected",
                                role = "receiver",
                                peer = %peer_id.0,
                                claimed_primary = %primary_device_id.0,
                                generation = generation,
                                reason = "invalid_target_owner",
                                "ignored an OwnershipChanged naming a device outside this pair"
                            );
                            continue;
                        }
                        if !ownership.try_advance_generation(generation) {
                            crate::hop_note!(
                                stage = "ownership_rejected",
                                role = "receiver",
                                peer = %peer_id.0,
                                generation = generation,
                                reason = "stale_or_duplicate_generation",
                                "ignored an OwnershipChanged at or below the last accepted generation"
                            );
                            continue;
                        }
                        let now_forwarding = primary_device_id == local_id;
                        let my_role = role_of(now_forwarding);
                        crate::hop_note!(
                            stage = "switch_key",
                            role = "receiver",
                            peer = %peer_id.0,
                            primary_device_id = %primary_device_id.0,
                            new_role = ?my_role,
                            generation = generation,
                            "peer handed the ownership baton over the live connection"
                        );
                        // Were forwarding, now Secondary: flush what we
                        // had mid-forward so the peer isn't left holding a
                        // key we will never release (task §11).
                        if forwarding && !now_forwarding
                            && forward_releases(&mut channel, &mut send_sequence, sent_held.drain_releases()).await.is_err()
                        {
                            break 'conn;
                        }
                        // Release anything we injected as the former
                        // Secondary — the peer that pressed those keys is
                        // no longer our input source.
                        held.release_all(&mut injector);
                        // Apply immediately so the send gate is correct in
                        // the gap before `on_peer_ownership`'s device-list
                        // update lands; that update then matches
                        // `forwarding` and is a no-op in the `devices`
                        // arm, so it is never echoed back as a second
                        // handoff.
                        forwarding = now_forwarding;
                        ownership.set_role(my_role);
                        suppress_local(forwarding);
                        on_peer_ownership(my_role);
                    }
                    Ok(_) => continue,
                    Err(_) => break 'conn,
                }
            }
        }
    }
    held.release_all(&mut injector);
    // Never leave this machine's own input suppressed once the
    // connection it was being forwarded over is gone — otherwise a
    // dropped link would take the user's keyboard with it.
    if forwarding {
        suppress_local(false);
    }
    // Deliberately leaves `ownership`'s role untouched: forcing it back to
    // `Primary` here — regardless of what it actually was — is exactly what
    // produced split-brain ownership, since both peers run this same
    // teardown independently on the same disconnect. The next connection's
    // opening handshake (top of this function) reconciles it instead.
}

/// This side's role given whether it is currently forwarding captured
/// input to the peer.
fn role_of(forwarding: bool) -> InputRole {
    if forwarding {
        InputRole::Primary
    } else {
        InputRole::Secondary
    }
}

/// The absolute id of whichever device this side currently believes is
/// Primary — this daemon itself, or `peer_id`. This is what every
/// `ChannelMessage::OwnershipChanged` this side sends carries, replacing a
/// sender-relative role a receiver would otherwise have to invert
/// (`sender_role.opposite()`) — the exact inference the "Fix Flow V1
/// Ownership Synchronization" task rules out, since it can't tell two
/// independently-diverged peers apart.
fn local_belief(ownership: &crate::ownership::OwnershipHandle, peer_id: &DeviceId) -> DeviceId {
    if ownership.is_primary() {
        DeviceId(LOCAL_DEVICE_ID.to_string())
    } else {
        peer_id.clone()
    }
}

/// Whether `id` names this daemon itself (as opposed to its peer).
fn is_local_device(id: &DeviceId) -> bool {
    id.0 == LOCAL_DEVICE_ID
}

/// Deterministically resolves two peers' independent beliefs about who is
/// Primary into the one answer both sides must agree on. This is the
/// mechanism behind both "deterministic initial owner" and "no split-brain
/// after a reconnect" — the same function serves both, since a first-ever
/// connection is just a reconnect where neither side has ever handed the
/// baton yet (generation 0 on both ends).
///
/// - A strictly higher `generation` is fresher and wins outright, no matter
///   which side (local or peer) reports it — direction-independent by
///   construction (swap every `local_*`/`peer_*` argument and the same
///   absolute device id still wins).
/// - An exact tie where both sides already name the same primary isn't a
///   conflict at all — kept as-is.
/// - An exact tie where they *disagree* — including two fresh daemons'
///   first-ever connection, where both default to "I am Primary of
///   myself" at generation 0 — is broken by comparing device ids: the
///   lexicographically smaller one is Primary. Both sides compare the same
///   two ids, so both always reach the same verdict.
///
/// Returns `(resolved_primary_device_id, resolved_generation)`; the caller
/// applies this via `OwnershipHandle::reconcile`.
fn resolve_ownership(
    local_id: &DeviceId,
    local_primary: &DeviceId,
    local_generation: u64,
    peer_id: &DeviceId,
    peer_primary: &DeviceId,
    peer_generation: u64,
) -> (DeviceId, u64) {
    let resolved_generation = local_generation.max(peer_generation);
    let resolved_primary = match local_generation.cmp(&peer_generation) {
        std::cmp::Ordering::Greater => local_primary.clone(),
        std::cmp::Ordering::Less => peer_primary.clone(),
        std::cmp::Ordering::Equal if local_primary == peer_primary => local_primary.clone(),
        std::cmp::Ordering::Equal => {
            if local_id.0 < peer_id.0 {
                local_id.clone()
            } else {
                peer_id.clone()
            }
        }
    };
    (resolved_primary, resolved_generation)
}

/// Forwards a batch of synthesized release events to the peer as ordinary
/// sequenced `Input` frames. `Err(())` if the channel send fails, so the
/// caller can end the connection the same way any other send failure does.
async fn forward_releases(
    channel: &mut Box<dyn Channel>,
    send_sequence: &mut u64,
    releases: Vec<InputEvent>,
) -> Result<(), ()> {
    for event in releases {
        *send_sequence += 1;
        if channel
            .send(ChannelMessage::Input {
                sequence: *send_sequence,
                event,
            })
            .await
            .is_err()
        {
            return Err(());
        }
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A short, stable label for an event, for the `kind` field on
/// `flow::hop` records — enough to eyeball "keydown A" vs "mouse move"
/// in a log without dumping the whole `InputEvent`. `pub` so `main.rs`'s
/// capture-bridge hop can use the same labels.
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

/// A short, human-readable description of one event for the `[INPUT]`
/// product log line — "KeyDown A", "MouseMove dx=12 dy=-4", "LeftClick
/// down", "Scroll dx=0 dy=-3". Unlike [`event_kind`]'s machine-stable
/// token, this is written for a person eyeballing the log.
fn describe_event(event: &InputEvent) -> String {
    match event {
        InputEvent::Keyboard(KeyboardEvent::KeyDown { key, .. }) => format!("KeyDown {key}"),
        InputEvent::Keyboard(KeyboardEvent::KeyUp { key, .. }) => format!("KeyUp {key}"),
        InputEvent::Mouse(MouseEvent::Move { dx, dy, .. }) => format!("MouseMove dx={dx} dy={dy}"),
        InputEvent::Mouse(MouseEvent::ButtonDown { button, .. }) => {
            format!("{} down", mouse_button_label(button))
        }
        InputEvent::Mouse(MouseEvent::ButtonUp { button, .. }) => {
            format!("{} up", mouse_button_label(button))
        }
        InputEvent::Mouse(MouseEvent::Scroll { dx, dy, .. }) => format!("Scroll dx={dx} dy={dy}"),
    }
}

fn mouse_button_label(button: &MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "LeftClick",
        MouseButton::Right => "RightClick",
        MouseButton::Middle => "MiddleClick",
    }
}

/// Resolves a device id to its display name from the current `devices`
/// snapshot, falling back to the raw id when the device isn't listed
/// (e.g. it was just unpaired).
fn device_name(devices: &[Device], id: &str) -> String {
    devices
        .iter()
        .find(|device| device.id.0 == id)
        .map(|device| device.name.clone())
        .unwrap_or_else(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::tcp::TcpChannel;
    use crate::ownership::OwnershipHandle;
    use crate::service::LOCAL_DEVICE_ID;
    use flow_core::device::HostOs;
    use flow_core::protocol::{InputEvent, KeyboardEvent};
    use tokio::net::TcpListener;

    fn a_key_event(key: &str) -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: key.to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        })
    }

    fn local_device(state: DeviceState) -> Device {
        Device {
            id: DeviceId(LOCAL_DEVICE_ID.to_string()),
            name: "This Machine".to_string(),
            os: HostOs::Linux,
            state,
            last_seen: chrono::Utc::now(),
        }
    }

    /// The remote device each pipeline test is connected to.
    const PEER_ID: &str = "peer-1";

    fn peer_id() -> DeviceId {
        DeviceId(PEER_ID.to_string())
    }

    fn peer_device(state: DeviceState) -> Device {
        Device {
            id: peer_id(),
            name: "Peer".to_string(),
            os: HostOs::Linux,
            state,
            last_seen: chrono::Utc::now(),
        }
    }

    /// The two-device world every streaming test runs in: exactly one of
    /// the pair is `Active`, matching the real invariant
    /// `DaemonService::switch_active_device` maintains.
    fn devices_with_active_peer() -> Vec<Device> {
        vec![
            local_device(DeviceState::Inactive),
            peer_device(DeviceState::Active),
        ]
    }

    fn devices_with_active_local() -> Vec<Device> {
        vec![
            local_device(DeviceState::Active),
            peer_device(DeviceState::Inactive),
        ]
    }

    /// A `suppress_local` sink for tests that don't assert on it.
    fn ignore_suppression() -> impl FnMut(bool) {
        |_| {}
    }

    /// An `on_peer_ownership` sink for tests that don't assert on it.
    fn ignore_ownership() -> impl FnMut(InputRole) {
        |_| {}
    }

    async fn connected_pair() -> (Box<dyn Channel>, Box<dyn Channel>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            let (stream, _peer) = listener.accept().await.expect("accept");
            TcpChannel::accept(stream).await.expect("accept ws")
        });
        let client = TcpChannel::connect(addr).await.expect("connect");
        let server = server.await.expect("server task");
        (Box::new(client), Box::new(server))
    }

    /// Drives the peer side of the ownership-reconciliation handshake every
    /// `run_paired_connection` now performs before its main loop: consumes
    /// the pipeline's own opening `OwnershipChanged`, then answers with
    /// `claimed_primary`/`claimed_generation` so the two sides reconcile to
    /// a known, chosen starting state instead of each test having to
    /// hand-roll the exchange. Must be called (and awaited) before any
    /// other traffic on `peer_side`, exactly once per connection.
    async fn handshake_as_peer(
        peer_side: &mut Box<dyn Channel>,
        claimed_primary: DeviceId,
        claimed_generation: u64,
    ) {
        match peer_side
            .recv()
            .await
            .expect("recv the pipeline's opening handshake message")
        {
            ChannelMessage::OwnershipChanged { .. } => {}
            other => panic!("expected the pipeline's opening OwnershipChanged, got {other:?}"),
        }
        peer_side
            .send(ChannelMessage::OwnershipChanged {
                primary_device_id: claimed_primary,
                generation: claimed_generation,
            })
            .await
            .expect("send handshake reply");
    }

    /// Answers the opening handshake claiming the peer itself already
    /// agrees this side (the pipeline under test) is Primary — the pipeline
    /// starts forwarding, generation 0 on both ends (an uncontested tie:
    /// both name the same primary, so no tie-break is even reached).
    async fn handshake_starting_primary(peer_side: &mut Box<dyn Channel>) {
        handshake_as_peer(peer_side, DeviceId(LOCAL_DEVICE_ID.to_string()), 0).await;
    }

    /// Answers the opening handshake claiming the peer itself is Primary at
    /// a generation strictly higher than the pipeline's own default (0) —
    /// its claim wins outright, so the pipeline under test starts Secondary.
    async fn handshake_starting_secondary(peer_side: &mut Box<dyn Channel>) {
        handshake_as_peer(peer_side, peer_id(), 1).await;
    }

    /// The direction that matters, and the one this pipeline previously
    /// had backwards: input is forwarded to a peer exactly when *that
    /// peer* is the active device (`vision.md` §22, "only the active
    /// device should receive input") — never when this machine is the
    /// active one, which is the case where input should stay put.
    #[test]
    fn input_is_forwarded_only_while_the_peer_is_the_active_device() {
        assert!(is_peer_receiving_input(
            &devices_with_active_peer(),
            &peer_id()
        ));
        assert!(!is_peer_receiving_input(
            &devices_with_active_local(),
            &peer_id()
        ));
        assert!(!is_peer_receiving_input(&[], &peer_id()));
    }

    /// With three devices paired and a third one active, this
    /// connection's peer is *not* the destination — so nothing goes down
    /// this channel, rather than every peer receiving a copy.
    #[test]
    fn input_is_not_forwarded_to_a_peer_when_a_different_device_is_active() {
        let devices = vec![
            local_device(DeviceState::Inactive),
            peer_device(DeviceState::Inactive),
            Device {
                id: DeviceId("other-peer".to_string()),
                name: "Third Machine".to_string(),
                os: HostOs::Linux,
                state: DeviceState::Active,
                last_seen: chrono::Utc::now(),
            },
        ];
        assert!(!is_peer_receiving_input(&devices, &peer_id()));
        assert!(is_peer_receiving_input(
            &devices,
            &DeviceId("other-peer".to_string())
        ));
    }

    fn local_id() -> DeviceId {
        DeviceId(LOCAL_DEVICE_ID.to_string())
    }

    /// A strictly higher generation wins outright — and does so
    /// identically regardless of which side of the call is "local" and
    /// which is "peer": swapping every `local_*`/`peer_*` argument still
    /// picks the same absolute device id, which is what makes ownership
    /// resolution independent of connection direction.
    #[test]
    fn resolve_ownership_picks_the_strictly_higher_generation_regardless_of_direction() {
        let (primary, generation) =
            resolve_ownership(&local_id(), &local_id(), 5, &peer_id(), &peer_id(), 2);
        assert_eq!(primary, local_id());
        assert_eq!(generation, 5);

        // Same facts, roles swapped: the peer's perspective must reach the
        // identical absolute answer.
        let (primary, generation) =
            resolve_ownership(&peer_id(), &peer_id(), 2, &local_id(), &local_id(), 5);
        assert_eq!(
            primary,
            local_id(),
            "the higher-generation claimant is the same absolute device either way"
        );
        assert_eq!(generation, 5);
    }

    /// An exact generation tie where both sides already name the same
    /// primary isn't a conflict — it's kept as-is.
    #[test]
    fn resolve_ownership_tie_with_agreement_is_a_no_op() {
        let (primary, generation) =
            resolve_ownership(&local_id(), &peer_id(), 3, &peer_id(), &peer_id(), 3);
        assert_eq!(primary, peer_id());
        assert_eq!(generation, 3);
    }

    /// The deterministic tie-break: two fresh daemons' first-ever
    /// connection (generation 0 on both sides, each still defaulting to
    /// "I am Primary of myself") must not leave both sides claiming
    /// Primary — the lexicographically smaller device id wins, and both
    /// sides of the same pair compute the identical verdict.
    #[test]
    fn resolve_ownership_breaks_a_disagreeing_tie_deterministically_and_symmetrically() {
        let (primary, generation) =
            resolve_ownership(&local_id(), &local_id(), 0, &peer_id(), &peer_id(), 0);
        assert_eq!(generation, 0);
        let expected = if local_id().0 < peer_id().0 {
            local_id()
        } else {
            peer_id()
        };
        assert_eq!(primary, expected);

        // The peer's own perspective on the identical facts must resolve
        // to the same absolute device.
        let (primary_from_peer, _) =
            resolve_ownership(&peer_id(), &peer_id(), 0, &local_id(), &local_id(), 0);
        assert_eq!(
            primary_from_peer, expected,
            "both sides of the same pair must reach the same absolute Primary"
        );
    }

    #[tokio::test]
    async fn an_event_captured_while_active_is_streamed_to_the_peer() {
        let (sender_side, mut receiver_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(devices_with_active_peer());
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();

        let pipeline = tokio::spawn(send_while_active(
            capture_rx,
            devices_rx,
            sender_side,
            peer_id(),
        ));

        let event = a_key_event("A");
        capture_tx.send(event.clone()).expect("send captured event");
        let received = receiver_side.recv().await.expect("recv");
        assert_eq!(received, ChannelMessage::Input { sequence: 1, event });

        drop(capture_tx);
        drop(devices_tx);
        pipeline.await.expect("pipeline task");
    }

    #[tokio::test]
    async fn an_event_captured_while_inactive_is_dropped_not_streamed() {
        let (sender_side, mut receiver_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(devices_with_active_local());
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();

        let pipeline = tokio::spawn(send_while_active(
            capture_rx,
            devices_rx,
            sender_side,
            peer_id(),
        ));

        capture_tx
            .send(a_key_event("dropped"))
            .expect("send captured event while this machine is the active one");
        // Closing the capture channel (rather than racing a state flip
        // against an unsynchronized second send) is what makes this
        // deterministic: `send_while_active`'s loop drains every queued
        // capture event — applying the gate to each in turn — before its
        // `recv()` branch finally yields `None` and the loop, and the
        // `Channel` it owns, both drop.
        drop(capture_tx);
        drop(devices_tx);
        pipeline.await.expect("pipeline task");

        // If "dropped" had been sent despite being captured while
        // Inactive, it would arrive before this — proving the gate
        // actually suppressed it, not just that nothing happened to
        // race in yet.
        assert_eq!(
            receiver_side.recv().await,
            Err(flow_core::channel::ChannelError::ConnectionLost)
        );
    }

    /// A minimal `InputInjector` test double: records every event handed
    /// to it instead of touching real hardware. Uses an async channel
    /// (unbounded `send` is synchronous, so this still fits the
    /// synchronous `InputInjector::inject` signature) rather than
    /// `std::sync::mpsc` — a blocking `recv()` on that from within a
    /// `#[tokio::test]`'s single-threaded runtime would starve the very
    /// executor the spawned `receive_and_inject` task needs to run on,
    /// deadlocking the test.
    struct RecordingInjector {
        received: mpsc::UnboundedSender<InputEvent>,
    }

    impl InputInjector for RecordingInjector {
        type Error = std::convert::Infallible;

        fn inject(&mut self, event: &InputEvent) -> Result<(), Self::Error> {
            let _ = self.received.send(event.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_received_input_message_is_handed_to_the_injector() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        let event = a_key_event("Z");
        sender_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: event.clone(),
            })
            .await
            .expect("send");

        let injected = rx.recv().await.expect("injector received the event");
        assert_eq!(injected, event);

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");
    }

    #[tokio::test]
    async fn a_non_input_message_is_ignored_not_injected() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        sender_side
            .send(ChannelMessage::Heartbeat)
            .await
            .expect("send heartbeat");
        let event = a_key_event("after heartbeat");
        sender_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: event.clone(),
            })
            .await
            .expect("send input");

        let injected = rx.recv().await.expect("injector received the input event");
        assert_eq!(injected, event);
        assert!(rx.try_recv().is_err(), "the heartbeat must not be injected");

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");
    }

    #[tokio::test]
    async fn a_duplicate_sequence_frame_is_dropped_not_injected() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        // Mouse::Move deliberately, not a KeyDown: that isolates this
        // test to sequence-based replay dropping alone, with no
        // held-key release (a KeyDown never released before disconnect
        // would itself inject a synthesized KeyUp — a real and separately
        // tested behavior, see `a_key_held_when_the_connection_drops_is_released_not_left_stuck`,
        // just not what this test is about).
        let first = InputEvent::Mouse(MouseEvent::Move {
            dx: 1,
            dy: 1,
            timestamp_ms: 0,
        });
        sender_side
            .send(ChannelMessage::Input {
                sequence: 5,
                event: first.clone(),
            })
            .await
            .expect("send first");
        assert_eq!(rx.recv().await.expect("first event injected"), first);

        // Same sequence as the already-accepted message - a replayed
        // frame per H4's guard, must be dropped rather than injected,
        // even though its own timestamp_ms differs (proving the check is
        // on sequence, not timestamp).
        let replay = InputEvent::Mouse(MouseEvent::Move {
            dx: 99,
            dy: 99,
            timestamp_ms: 999,
        });
        sender_side
            .send(ChannelMessage::Input {
                sequence: 5,
                event: replay,
            })
            .await
            .expect("send replay");

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        assert!(
            rx.try_recv().is_err(),
            "the replayed frame must not have been injected"
        );
    }

    #[tokio::test]
    async fn an_out_of_order_lower_sequence_frame_is_dropped_not_injected() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        let first = a_key_event("first");
        sender_side
            .send(ChannelMessage::Input {
                sequence: 10,
                event: first.clone(),
            })
            .await
            .expect("send first");
        assert_eq!(rx.recv().await.expect("first event injected"), first);

        // Lower sequence than the last accepted message - dropped, not
        // injected, regardless of its own timestamp_ms.
        let stale = a_key_event("stale");
        sender_side
            .send(ChannelMessage::Input {
                sequence: 7,
                event: stale,
            })
            .await
            .expect("send stale");

        let next = a_key_event("next");
        sender_side
            .send(ChannelMessage::Input {
                sequence: 11,
                event: next.clone(),
            })
            .await
            .expect("send next");

        // The next value this channel yields is whatever was actually
        // injected - if the stale frame had slipped through, this would
        // be it instead of `next`, proving the drop deterministically
        // rather than by racing a timeout against nothing arriving.
        assert_eq!(rx.recv().await.expect("next event injected"), next);

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");
    }

    #[tokio::test]
    async fn send_while_active_assigns_strictly_increasing_sequence_numbers() {
        let (sender_side, mut receiver_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(devices_with_active_peer());
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();

        let pipeline = tokio::spawn(send_while_active(
            capture_rx,
            devices_rx,
            sender_side,
            peer_id(),
        ));

        capture_tx.send(a_key_event("one")).expect("send 1");
        capture_tx.send(a_key_event("two")).expect("send 2");
        capture_tx.send(a_key_event("three")).expect("send 3");

        // A single sender task processing an unbounded (FIFO) channel:
        // three sequential receives, in send order, are exactly what
        // arrives — no concurrency to coordinate here.
        let mut sequences = Vec::new();
        for _ in 0..3 {
            match receiver_side.recv().await.expect("recv") {
                ChannelMessage::Input { sequence, .. } => sequences.push(sequence),
                other => panic!("expected an Input message, got {other:?}"),
            }
        }
        assert_eq!(sequences, vec![1, 2, 3]);

        drop(capture_tx);
        drop(devices_tx);
        pipeline.await.expect("pipeline task");
    }

    fn key_down(key: &str) -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyDown {
            key: key.to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        })
    }

    fn key_up(key: &str) -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent::KeyUp {
            key: key.to_string(),
            modifiers: vec![],
            timestamp_ms: 0,
        })
    }

    fn button_down(button: MouseButton) -> InputEvent {
        InputEvent::Mouse(MouseEvent::ButtonDown {
            button,
            timestamp_ms: 0,
        })
    }

    /// Reads whatever the injector received next, expecting a `KeyUp`/
    /// `ButtonUp` — the synthesized release, since nothing else in these
    /// tests injects one directly.
    async fn expect_next_is_key_up(rx: &mut mpsc::UnboundedReceiver<InputEvent>, key: &str) {
        match rx.recv().await.expect("a release event") {
            InputEvent::Keyboard(KeyboardEvent::KeyUp { key: released, .. }) => {
                assert_eq!(released, key);
            }
            other => panic!("expected a synthesized KeyUp for {key:?}, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_key_held_when_the_connection_drops_is_released_not_left_stuck() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        sender_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: key_down("A"),
            })
            .await
            .expect("send keydown");
        assert_eq!(rx.recv().await.expect("keydown injected"), key_down("A"));

        // The connection drops with no matching KeyUp ever sent — exactly
        // the "key_down sent, connection drops, key_up never arrives"
        // scenario the review calls out.
        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        expect_next_is_key_up(&mut rx, "A").await;
        assert!(
            rx.try_recv().is_err(),
            "exactly one synthesized release, nothing extra"
        );
    }

    #[tokio::test]
    async fn a_key_already_released_before_disconnect_is_not_released_again() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        sender_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: key_down("A"),
            })
            .await
            .expect("send keydown");
        assert_eq!(rx.recv().await.expect("keydown injected"), key_down("A"));

        sender_side
            .send(ChannelMessage::Input {
                sequence: 2,
                event: key_up("A"),
            })
            .await
            .expect("send keyup");
        assert_eq!(rx.recv().await.expect("keyup injected"), key_up("A"));

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        assert!(
            rx.try_recv().is_err(),
            "a key already released normally must not get a second, spurious release"
        );
    }

    #[tokio::test]
    async fn a_held_mouse_button_when_the_connection_drops_is_released() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        sender_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: button_down(MouseButton::Left),
            })
            .await
            .expect("send button down");
        assert_eq!(
            rx.recv().await.expect("button down injected"),
            button_down(MouseButton::Left)
        );

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        match rx.recv().await.expect("a release event") {
            InputEvent::Mouse(MouseEvent::ButtonUp { button, .. }) => {
                assert_eq!(button, MouseButton::Left);
            }
            other => panic!("expected a synthesized ButtonUp, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn multiple_held_keys_are_all_released_on_disconnect() {
        let (mut sender_side, receiver_side) = connected_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(receive_and_inject(receiver_side, injector));

        for (sequence, key) in [(1, "Ctrl"), (2, "Shift"), (3, "A")] {
            sender_side
                .send(ChannelMessage::Input {
                    sequence,
                    event: key_down(key),
                })
                .await
                .expect("send keydown");
            assert_eq!(rx.recv().await.expect("keydown injected"), key_down(key));
        }

        sender_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        let mut released = std::collections::HashSet::new();
        for _ in 0..3 {
            match rx.recv().await.expect("a release event") {
                InputEvent::Keyboard(KeyboardEvent::KeyUp { key, .. }) => {
                    released.insert(key);
                }
                other => panic!("expected a synthesized KeyUp, got {other:?}"),
            }
        }
        assert_eq!(
            released,
            ["Ctrl", "Shift", "A"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
    }

    #[tokio::test]
    async fn run_paired_connection_handles_both_directions_over_one_connection() {
        let (mut peer_side, our_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(devices_with_active_peer());
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(run_paired_connection(
            our_side,
            capture_rx,
            devices_rx,
            injector,
            peer_id(),
            ignore_suppression(),
            ignore_ownership(),
            OwnershipHandle::new(),
        ));
        handshake_starting_primary(&mut peer_side).await;

        // The peer is the active device, so captured input is forwarded
        // to it rather than staying on this machine.
        let outgoing = a_key_event("A");
        capture_tx
            .send(outgoing.clone())
            .expect("send captured event");
        let received_by_peer = peer_side.recv().await.expect("recv");
        assert_eq!(
            received_by_peer,
            ChannelMessage::Input {
                sequence: 1,
                event: outgoing
            }
        );

        // The peer sends something back (as if it just became active
        // itself) — our side must inject it, over this same connection,
        // proving both directions share one `run_paired_connection` task
        // rather than needing two separate channels.
        let incoming = a_key_event("Z");
        peer_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: incoming.clone(),
            })
            .await
            .expect("send");
        let injected = rx.recv().await.expect("injector received the event");
        assert_eq!(injected, incoming);

        drop(capture_tx);
        drop(devices_tx);
        peer_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");
    }

    #[tokio::test]
    async fn run_paired_connection_releases_held_input_when_the_connection_drops() {
        let (mut peer_side, our_side) = connected_pair().await;
        let (_devices_tx, devices_rx) = watch::channel(devices_with_active_local());
        let (_capture_tx, capture_rx) = mpsc::unbounded_channel();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let pipeline = tokio::spawn(run_paired_connection(
            our_side,
            capture_rx,
            devices_rx,
            injector,
            peer_id(),
            ignore_suppression(),
            ignore_ownership(),
            OwnershipHandle::new(),
        ));
        handshake_starting_secondary(&mut peer_side).await;

        peer_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: key_down("A"),
            })
            .await
            .expect("send keydown");
        assert_eq!(rx.recv().await.expect("keydown injected"), key_down("A"));

        peer_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        expect_next_is_key_up(&mut rx, "A").await;
    }

    /// Local input must be suppressed exactly while it's being forwarded
    /// away, and released again when the connection ends — otherwise a
    /// dropped link would leave the user's own keyboard grabbed.
    #[tokio::test]
    async fn local_input_is_suppressed_while_forwarding_and_released_on_disconnect() {
        let (mut peer_side, our_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(devices_with_active_peer());
        let (_capture_tx, capture_rx) = mpsc::unbounded_channel();
        let (tx, _rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let (suppress_tx, mut suppress_rx) = mpsc::unbounded_channel();
        let pipeline = tokio::spawn(run_paired_connection(
            our_side,
            capture_rx,
            devices_rx,
            injector,
            peer_id(),
            move |suppress| {
                let _ = suppress_tx.send(suppress);
            },
            ignore_ownership(),
            OwnershipHandle::new(),
        ));
        handshake_starting_primary(&mut peer_side).await;

        // The peer is already active when the connection opens, so
        // suppression is applied immediately rather than only on the
        // next switch.
        assert_eq!(suppress_rx.recv().await, Some(true));

        // Switching back to this machine releases it: input is no longer
        // being forwarded, so it must reach local applications again.
        devices_tx.send_replace(devices_with_active_local());
        assert_eq!(suppress_rx.recv().await, Some(false));

        // ...and switching away re-applies it.
        devices_tx.send_replace(devices_with_active_peer());
        assert_eq!(suppress_rx.recv().await, Some(true));

        peer_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        // The connection ended while suppressing — the final call must
        // hand local input back.
        assert_eq!(suppress_rx.recv().await, Some(false));
    }

    /// The complement: a connection that never suppressed anything
    /// shouldn't emit a spurious release on the way out.
    #[tokio::test]
    async fn a_connection_that_never_suppressed_does_not_release_on_disconnect() {
        let (mut peer_side, our_side) = connected_pair().await;
        let (_devices_tx, devices_rx) = watch::channel(devices_with_active_local());
        let (_capture_tx, capture_rx) = mpsc::unbounded_channel();
        let (tx, _rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: tx };

        let (suppress_tx, mut suppress_rx) = mpsc::unbounded_channel();
        let pipeline = tokio::spawn(run_paired_connection(
            our_side,
            capture_rx,
            devices_rx,
            injector,
            peer_id(),
            move |suppress| {
                let _ = suppress_tx.send(suppress);
            },
            ignore_ownership(),
            OwnershipHandle::new(),
        ));
        handshake_starting_secondary(&mut peer_side).await;

        // One initial `false` for the starting state, then nothing.
        assert_eq!(suppress_rx.recv().await, Some(false));

        peer_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        assert_eq!(
            suppress_rx.recv().await,
            None,
            "no further suppression calls once the channel's sender drops"
        );
    }

    /// Mimics `DaemonService::apply_peer_ownership` for the pipeline
    /// tests: an `on_peer_ownership` callback that flips a `devices` watch
    /// to match the role the peer just handed this side.
    fn apply_ownership_to(devices_tx: watch::Sender<Vec<Device>>) -> impl FnMut(InputRole) {
        move |role| {
            let list = if role == InputRole::Primary {
                devices_with_active_peer()
            } else {
                devices_with_active_local()
            };
            devices_tx.send_replace(list);
        }
    }

    /// The receiving half of the ownership baton: a peer's
    /// `OwnershipChanged` making this side `Primary` must start forwarding
    /// captured input and suppress it locally — over the same connection,
    /// no reconnect.
    #[tokio::test]
    async fn a_received_switch_ownership_making_this_side_primary_starts_forwarding() {
        let (mut peer_side, our_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(devices_with_active_local());
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();
        let (inj_tx, _inj_rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: inj_tx };
        let (suppress_tx, mut suppress_rx) = mpsc::unbounded_channel();

        let pipeline = tokio::spawn(run_paired_connection(
            our_side,
            capture_rx,
            devices_rx,
            injector,
            peer_id(),
            move |suppress| {
                let _ = suppress_tx.send(suppress);
            },
            apply_ownership_to(devices_tx),
            OwnershipHandle::new(),
        ));
        handshake_starting_secondary(&mut peer_side).await;

        // Starts Secondary.
        assert_eq!(suppress_rx.recv().await, Some(false));

        // The peer says this side is now Primary. Generation 2: the
        // handshake above already raised the floor to 1
        // (`handshake_starting_secondary`), so this live handoff must be
        // strictly higher to be accepted as a fresh change rather than a
        // stale/duplicate one.
        peer_side
            .send(ChannelMessage::OwnershipChanged {
                primary_device_id: DeviceId(LOCAL_DEVICE_ID.to_string()),
                generation: 2,
            })
            .await
            .expect("send handoff");
        assert_eq!(suppress_rx.recv().await, Some(true));

        // Captured input now flows to the peer.
        capture_tx.send(a_key_event("A")).expect("send capture");
        match peer_side.recv().await.expect("recv") {
            ChannelMessage::Input { event, .. } => assert_eq!(event, a_key_event("A")),
            other => panic!("expected a forwarded Input frame, got {other:?}"),
        }

        drop(capture_tx);
        peer_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");
    }

    /// A switch initiated on *this* machine (a `devices` change) while a
    /// key is held forwarded: the peer must get the synthesized release
    /// first, then the `OwnershipChanged` handoff — never a stuck key.
    #[tokio::test]
    async fn a_local_switch_flushes_held_forwarded_input_then_relays_the_handoff() {
        let (mut peer_side, our_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(devices_with_active_peer());
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();
        let (inj_tx, _inj_rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: inj_tx };

        let pipeline = tokio::spawn(run_paired_connection(
            our_side,
            capture_rx,
            devices_rx,
            injector,
            peer_id(),
            ignore_suppression(),
            ignore_ownership(),
            OwnershipHandle::new(),
        ));
        handshake_starting_primary(&mut peer_side).await;

        capture_tx.send(key_down("A")).expect("send keydown");
        assert_eq!(
            peer_side.recv().await.expect("recv"),
            ChannelMessage::Input {
                sequence: 1,
                event: key_down("A")
            }
        );

        // Switch control back to this machine.
        devices_tx.send_replace(devices_with_active_local());

        match peer_side.recv().await.expect("recv") {
            ChannelMessage::Input {
                sequence,
                event: InputEvent::Keyboard(KeyboardEvent::KeyUp { key, .. }),
            } => {
                assert_eq!(sequence, 2, "the flushed release keeps the sequence going");
                assert_eq!(key, "A");
            }
            other => panic!("expected a synthesized KeyUp frame first, got {other:?}"),
        }
        match peer_side.recv().await.expect("recv") {
            ChannelMessage::OwnershipChanged {
                primary_device_id,
                generation,
            } => {
                assert_eq!(primary_device_id, peer_id());
                assert_eq!(
                    generation, 1,
                    "the first local-initiated handoff bumps generation to 1"
                );
            }
            other => panic!("expected the ownership handoff after the flush, got {other:?}"),
        }

        drop(capture_tx);
        drop(devices_tx);
        peer_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");
    }

    /// Applying a handoff the peer sent must not bounce a second
    /// `OwnershipChanged` straight back — that would ping-pong ownership
    /// forever.
    #[tokio::test]
    async fn a_peer_initiated_handoff_is_not_echoed_back() {
        let (mut peer_side, our_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(devices_with_active_local());
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();
        let (inj_tx, _inj_rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: inj_tx };
        let (suppress_tx, mut suppress_rx) = mpsc::unbounded_channel();

        let pipeline = tokio::spawn(run_paired_connection(
            our_side,
            capture_rx,
            devices_rx,
            injector,
            peer_id(),
            move |suppress| {
                let _ = suppress_tx.send(suppress);
            },
            apply_ownership_to(devices_tx),
            OwnershipHandle::new(),
        ));
        handshake_starting_secondary(&mut peer_side).await;

        // Starts Secondary.
        assert_eq!(suppress_rx.recv().await, Some(false));

        peer_side
            .send(ChannelMessage::OwnershipChanged {
                primary_device_id: DeviceId(LOCAL_DEVICE_ID.to_string()),
                generation: 2,
            })
            .await
            .expect("send handoff");
        // Synchronizes on the pipeline having actually processed the
        // handoff (suppression flips synchronously in that branch)
        // before sending the capture event below — otherwise `select!`
        // could poll the capture branch first, while `forwarding` is
        // still `false`, silently dropping the event (captured-while-
        // inactive is dropped, not queued) and hanging the test's later
        // `recv`.
        assert_eq!(suppress_rx.recv().await, Some(true));

        // The next frame the peer sees must be this side's forwarded
        // capture event, not an echoed handoff.
        capture_tx.send(a_key_event("Z")).expect("send capture");
        match peer_side.recv().await.expect("recv") {
            ChannelMessage::Input { event, .. } => assert_eq!(event, a_key_event("Z")),
            other => panic!("the peer's handoff was echoed back or reordered: {other:?}"),
        }

        drop(capture_tx);
        peer_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");
    }

    /// Held input this side had *injected* from the peer is released the
    /// moment the peer hands ownership away — the machine that pressed
    /// those keys is no longer the input source (task §11).
    #[tokio::test]
    async fn a_received_handoff_releases_input_this_side_had_injected() {
        let (mut peer_side, our_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(devices_with_active_local());
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();
        let (inj_tx, mut inj_rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: inj_tx };

        let pipeline = tokio::spawn(run_paired_connection(
            our_side,
            capture_rx,
            devices_rx,
            injector,
            peer_id(),
            ignore_suppression(),
            apply_ownership_to(devices_tx),
            OwnershipHandle::new(),
        ));
        handshake_starting_secondary(&mut peer_side).await;

        peer_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: key_down("A"),
            })
            .await
            .expect("send keydown");
        assert_eq!(
            inj_rx.recv().await.expect("keydown injected"),
            key_down("A")
        );

        peer_side
            .send(ChannelMessage::OwnershipChanged {
                primary_device_id: DeviceId(LOCAL_DEVICE_ID.to_string()),
                generation: 2,
            })
            .await
            .expect("send handoff");
        expect_next_is_key_up(&mut inj_rx, "A").await;

        drop(capture_tx);
        peer_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");
    }

    /// task §5's idempotent-ownership-update guard: an `OwnershipChanged`
    /// at or below the last accepted generation must be ignored
    /// entirely — no suppression call, no role change, no
    /// `on_peer_ownership` — not merely re-applied harmlessly.
    #[tokio::test]
    async fn a_stale_or_duplicate_ownership_generation_is_ignored() {
        let (mut peer_side, our_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(devices_with_active_local());
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();
        let (inj_tx, _inj_rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: inj_tx };
        let (suppress_tx, mut suppress_rx) = mpsc::unbounded_channel();
        let ownership = OwnershipHandle::new();

        let pipeline = tokio::spawn(run_paired_connection(
            our_side,
            capture_rx,
            devices_rx,
            injector,
            peer_id(),
            move |suppress| {
                let _ = suppress_tx.send(suppress);
            },
            apply_ownership_to(devices_tx),
            ownership,
        ));
        handshake_starting_secondary(&mut peer_side).await;

        // Starts Secondary.
        assert_eq!(suppress_rx.recv().await, Some(false));

        // A genuine handoff at generation 5 is accepted.
        peer_side
            .send(ChannelMessage::OwnershipChanged {
                primary_device_id: DeviceId(LOCAL_DEVICE_ID.to_string()),
                generation: 5,
            })
            .await
            .expect("send handoff");
        assert_eq!(suppress_rx.recv().await, Some(true));

        // A stale retransmit at a lower generation must be ignored: no
        // second suppression call, forwarding stays as it was.
        peer_side
            .send(ChannelMessage::OwnershipChanged {
                primary_device_id: DeviceId(LOCAL_DEVICE_ID.to_string()),
                generation: 3,
            })
            .await
            .expect("send stale handoff");
        // An exact duplicate of the already-accepted generation must
        // also be ignored.
        peer_side
            .send(ChannelMessage::OwnershipChanged {
                primary_device_id: DeviceId(LOCAL_DEVICE_ID.to_string()),
                generation: 5,
            })
            .await
            .expect("send duplicate handoff");

        // Prove neither stale message did anything: this side is still
        // forwarding (Primary), so a captured event still reaches the
        // peer rather than the connection having gone quiet or reset.
        capture_tx.send(a_key_event("A")).expect("send capture");
        match peer_side.recv().await.expect("recv") {
            ChannelMessage::Input { event, .. } => assert_eq!(event, a_key_event("A")),
            other => panic!("expected the forwarded Input frame, got {other:?}"),
        }

        drop(capture_tx);
        peer_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        // No further suppression calls were made by the ignored stale/
        // duplicate messages — only the final disconnect release.
        assert_eq!(suppress_rx.recv().await, Some(false));
        assert_eq!(suppress_rx.recv().await, None);
    }

    /// task's "invalid target owner rejected" requirement: an
    /// `OwnershipChanged` naming neither this daemon nor its peer must be
    /// ignored outright — no suppression call, no role change.
    #[tokio::test]
    async fn an_ownership_changed_naming_an_unknown_device_is_rejected() {
        let (mut peer_side, our_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(devices_with_active_local());
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();
        let (inj_tx, _inj_rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: inj_tx };
        let (suppress_tx, mut suppress_rx) = mpsc::unbounded_channel();

        let pipeline = tokio::spawn(run_paired_connection(
            our_side,
            capture_rx,
            devices_rx,
            injector,
            peer_id(),
            move |suppress| {
                let _ = suppress_tx.send(suppress);
            },
            apply_ownership_to(devices_tx),
            OwnershipHandle::new(),
        ));
        handshake_starting_secondary(&mut peer_side).await;

        // Starts Secondary.
        assert_eq!(suppress_rx.recv().await, Some(false));

        // Names a third device that isn't part of this pair at all.
        peer_side
            .send(ChannelMessage::OwnershipChanged {
                primary_device_id: DeviceId("some-other-device".to_string()),
                generation: 99,
            })
            .await
            .expect("send fabricated handoff");

        // Prove it did nothing: this side is still Secondary, so a
        // captured event stays local rather than being forwarded — and no
        // second suppression call was made by the rejected message.
        capture_tx.send(a_key_event("A")).expect("send capture");
        drop(capture_tx);
        peer_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        assert_eq!(
            suppress_rx.recv().await,
            None,
            "the rejected message must not have triggered any suppression change, including on disconnect"
        );
    }

    /// The direct regression guard for the fix: role is left as-is on
    /// disconnect rather than forced back to `Primary`, which is what
    /// produced split-brain (both peers independently reset to `Primary`
    /// on the same disconnect, with no way to reconcile afterward).
    #[tokio::test]
    async fn run_paired_connection_does_not_reset_role_on_disconnect() {
        let (mut peer_side, our_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(devices_with_active_peer());
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();
        let (inj_tx, mut inj_rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: inj_tx };
        let ownership = OwnershipHandle::new();
        let ownership_for_assert = ownership.clone();

        let pipeline = tokio::spawn(run_paired_connection(
            our_side,
            capture_rx,
            devices_rx,
            injector,
            peer_id(),
            ignore_suppression(),
            apply_ownership_to(devices_tx),
            ownership,
        ));
        handshake_starting_primary(&mut peer_side).await;
        assert!(ownership_for_assert.is_primary(), "starts Primary");

        // The peer becomes Primary, so this side becomes Secondary.
        peer_side
            .send(ChannelMessage::OwnershipChanged {
                primary_device_id: peer_id(),
                generation: 1,
            })
            .await
            .expect("send handoff");
        // Give the pipeline task a chance to apply it before asserting:
        // an injected event from the peer only arrives once this side
        // has processed the handoff.
        peer_side
            .send(ChannelMessage::Input {
                sequence: 1,
                event: a_key_event("Z"),
            })
            .await
            .expect("send input");
        inj_rx.recv().await.expect("event injected as Secondary");
        assert!(
            ownership_for_assert.is_secondary(),
            "peer's handoff made this side Secondary"
        );

        drop(capture_tx);
        peer_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        assert!(
            ownership_for_assert.is_secondary(),
            "disconnect must leave role exactly as it was — forcing Primary here is the split-brain bug"
        );
    }

    /// The end-to-end split-brain regression: A is Primary, hands off to
    /// B, the connection drops, and — instead of both sides guessing
    /// `Primary` independently, as the old unconditional reset did — a
    /// reconnect's opening handshake reconciles both sides back onto B,
    /// because B's persisted generation is the higher (fresher) one.
    #[tokio::test]
    async fn reconnect_after_disconnect_converges_on_the_peer_s_persisted_belief() {
        let ownership = OwnershipHandle::new();

        // --- First connection: A starts Primary, hands off to B, then the
        // link drops. ---
        {
            let (mut peer_side, our_side) = connected_pair().await;
            let (devices_tx, devices_rx) = watch::channel(devices_with_active_peer());
            let (capture_tx, capture_rx) = mpsc::unbounded_channel();
            let (inj_tx, mut inj_rx) = mpsc::unbounded_channel();
            let injector = RecordingInjector { received: inj_tx };

            let pipeline = tokio::spawn(run_paired_connection(
                our_side,
                capture_rx,
                devices_rx,
                injector,
                peer_id(),
                ignore_suppression(),
                apply_ownership_to(devices_tx),
                ownership.clone(),
            ));
            handshake_starting_primary(&mut peer_side).await;

            peer_side
                .send(ChannelMessage::OwnershipChanged {
                    primary_device_id: peer_id(),
                    generation: 1,
                })
                .await
                .expect("send handoff to B");
            // Synchronize on the pipeline having actually processed the
            // handoff before dropping the link — otherwise this side might
            // close the connection before it ever reads the message.
            peer_side
                .send(ChannelMessage::Input {
                    sequence: 1,
                    event: a_key_event("Z"),
                })
                .await
                .expect("send input to sync on the handoff");
            inj_rx.recv().await.expect("event injected as Secondary");
            drop(capture_tx);
            peer_side.close().await.expect("drop the link mid-session");
            pipeline.await.expect("pipeline task");
        }
        assert!(
            ownership.is_secondary(),
            "still Secondary immediately after the drop — the fix under test"
        );

        // --- Reconnect: B (the peer) still believes it is Primary at the
        // same generation this side accepted (1) — the persisted, fresher
        // belief. This side's own stale local generation is 0 (a fresh
        // `run_paired_connection` invocation reads it from the same,
        // unreset `ownership` handle). ---
        let (mut peer_side, our_side) = connected_pair().await;
        let (devices_tx, devices_rx) = watch::channel(devices_with_active_local());
        let (capture_tx, capture_rx) = mpsc::unbounded_channel();
        let (inj_tx, _inj_rx) = mpsc::unbounded_channel();
        let injector = RecordingInjector { received: inj_tx };

        let pipeline = tokio::spawn(run_paired_connection(
            our_side,
            capture_rx,
            devices_rx,
            injector,
            peer_id(),
            ignore_suppression(),
            apply_ownership_to(devices_tx),
            ownership.clone(),
        ));
        handshake_as_peer(&mut peer_side, peer_id(), 1).await;

        drop(capture_tx);
        peer_side.close().await.expect("close");
        pipeline.await.expect("pipeline task");

        assert!(
            ownership.is_secondary(),
            "reconnect must converge on B (the peer), not reset to Primary — both peers becoming \
             Primary on a reconnect is exactly the split-brain bug this pass fixes"
        );
    }
}
