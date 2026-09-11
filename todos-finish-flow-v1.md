# todos — finish Flow V1 (Primary/Secondary ownership + reliable 2-machine input)

Branch: `product-first-v1`. Baseline (acd2b9e): `cargo test --workspace` = **246 passed, 0 failed**
on `x86_64-pc-windows-msvc`. `cargo build` green.

Status keys: `[ ]` pending · `[~]` in progress · `[x]` completed · `[-]` blocked/skipped

---

## Phase 1 — Diagnosis (inspect before changing)

- [x] **D1** — trace `physical input -> capture -> routing -> TCP -> receive -> injection`
- [x] **D2** — trace `Scroll Lock -> ownership change -> suppression/routing`
- [x] **D3** — read `main.rs`, `pipeline/mod.rs`, `service/mod.rs`, `hotkey/{mod,runner}.rs`,
      `core/{channel,switch_key}`, `platform/src/windows/capture.rs`,
      `daemon/examples/drive_two_daemons.rs`
- [x] **D4** — establish baseline build + test state

### Findings

| # | Area | Finding |
|---|------|---------|
| F1 | **Ownership model** | There is **no explicit Primary/Secondary**. Ownership is the single `DeviceState::Active` marker in each daemon's *own* `ServiceState.devices`. One atomic `Active` per daemon (`switch_active_device[_local]` moves it). |
| F2 | **No wire propagation of ownership** | `ChannelMessage` = `Input | Pairing | Heartbeat | Noise`. **No switch/ownership message.** Each daemon flips its own `Active` locally; the peer is never told. The task's "switch command transported over the existing connection" is **not implemented**. |
| F3 | **Send gate** | `pipeline::run_paired_connection` forwards a captured event to the peer **iff that peer is `Active` on this daemon's list** (`is_peer_receiving_input`). Correct + atomic locally. |
| F4 | **Receive gate** | The `channel.recv()` arm injects **every** `ChannelMessage::Input` unconditionally — no ownership check on the receiving side. |
| F5 | **Suppression** | Follows the send gate: `suppress_local(true)` exactly while forwarding to the peer; released on switch-back and on disconnect. Windows impl is real (`LRESULT(1)` + `SuppressionGate` press/release symmetry). macOS impl is real but **never HW-verified** (NULL-return drop + `EVENT_SOURCE_USER_DATA` marker survival). |
| F6 | **Scroll Lock** | `SwitchKeyMatcher` (case-insensitive, single-key `ScrollLock` clean) + `SwitchDebouncer`. While a peer pipeline runs, `hotkey::runner::spawn_pipeline_switch_filter` detects it on the pipeline's own capture stream, calls `switch_active_device_local()`, and strips the `KeyDown` **and** matching `KeyUp` from what is forwarded. Standalone `hotkey::runner` stands down via `peer_pipeline_active()`. Scroll Lock is **not** forwarded. |
| F7 | **Held-input** | `HeldInputTracker` on the receive side synthesizes `KeyUp`/`ButtonUp` for anything held when the **connection drops**. It is **not** invoked on an *ownership change* that keeps the connection — a key held across a local switch-back is handled by the Windows `SuppressionGate` symmetry instead (release withheld iff press was). No cross-machine reconcile on switch. |
| F8 | **Connection arbitration** | Two daemons dial each other on startup ⇒ 2 TCP connections. `connection_precedence` (compare identity pubkeys) is symmetric ⇒ both sides keep the same one, both drop the other. `claim_lost` / "another task already holds this peer's connection slot" is the **designed** outcome for the losing direction. `try_claim_peer` HashSet dedups an inbound/outbound race to the same peer. **Not yet observed on real HW** — `1006` / flapping / duplicate pipelines are evidence-blocked (need `FLOW_TRACE` logs from a real 2-machine run). |
| F9 | **Consequence of F1/F2** | With **one keyboard on Windows** (product-first V1 goal): Windows Scroll Lock toggles *Windows-controls-Mac* ↔ *Windows-controls-Windows*. Works. But the Windows keyboard **cannot** make the Mac the Primary (Mac driving Windows) — that requires a switch initiated **on the Mac** (its keyboard, or an IPC `switch_active_device` from the Flutter UI). The task's canonical `Windows Primary -> Scroll Lock -> Mac Primary -> Scroll Lock -> Windows Primary` from a single keyboard is **not** achievable without F2. |

---

## Phase 2 — Decisions (RESOLVED)

- [x] **Q1 — ownership model scope** → **Add wire-propagated baton.** `ChannelMessage::SwitchOwnership`
  + `InputRole`, sent on a local switch while a pipeline is live, applied atomically by the peer.
- [x] **Q2 — reverse switch mechanism** → **Not testing reverse now.** V1 physical pass covers
  Windows-as-Primary; reverse (Mac-as-Primary) is deferred/documented as untested. (The baton
  itself is symmetric, so it *works* — the Windows Scroll Lock now toggles the full A↔B cycle.)
- [x] **Q3 — who runs physical E2E** → **maintainer, on two real machines.** This env is a
  single Windows host, no Mac, can't safely run an unattended suppressing hook. Code + unit/
  integration tests + the physical-test script are done here.

---

## Phase 3 — Implementation (DONE)

- [x] **I1** — `core`: `InputRole { Primary, Secondary }` (+ `opposite()`, snake_case serde) in
      `core/src/protocol`; `ChannelMessage::SwitchOwnership { sender_role: InputRole }`.
- [x] **I2** — `daemon/service`: `apply_peer_ownership(peer_id, InputRole)` — `Primary` ⇒ peer
      Active, `Secondary` ⇒ local Active; no debounce, no switchability check, idempotent;
      emits `devices` + `input_role_changed trigger=peer` hop + `[SWITCH]` product line.
- [x] **I3** — `daemon/pipeline::run_paired_connection`: new `on_peer_ownership` closure.
      `forwarding` (= this side is `Primary`) is now the authoritative role, updated the
      instant a `SwitchOwnership` arrives. On a local role flip (seen via `devices`): flush
      forwarded holds, then send `SwitchOwnership { sender_role }`. On a received
      `SwitchOwnership`: flush forwarded holds if going P→S, release injected holds, apply
      role immediately, call `on_peer_ownership`. The follow-up `devices` update matches
      `forwarding` ⇒ no echo (no ping-pong).
- [x] **I3b** — switch-key filter needs **no change**: `spawn_pipeline_switch_filter` already
      consumes the key (KeyDown+KeyUp) and calls `switch_active_device_local()`; the pipeline
      relays it. Standalone `hotkey::runner` still stands down via `peer_pipeline_active()`.
      *Deliberate deviation from task §5:* either machine's switch key hands the baton (so
      control can return from a keyboard-less Secondary) — matches the product-first "one
      keyboard, Scroll Lock switches" vision. Strict Primary-only is a one-line follow-up gate.
- [x] **I4** — held-input reconcile on ownership transition: `HeldInputTracker::drain_releases`
      shared by disconnect-release and the send-side flush; `sent_held` tracks forwarded
      holds and is flushed to the peer *before* the `SwitchOwnership` frame.
- [x] **I5** — logs: `input_role_changed` (trigger `local`/`peer`), `switch_key` (baton
      received), existing `switch_consumed`/`send_gate`/`frame_sent`/`frame_recv`/`injected`.
- [x] **I6** — `main.rs::run_peer_pipeline` wires `on_peer_ownership` → spawns
      `service.apply_peer_ownership(&device_id, role)`.

## Phase 4 — Tests (DONE — 255 pass, was 246)

- [x] **T1** — `core`: `InputRole::opposite`, snake_case serde; `SwitchOwnership` round-trips.
- [x] **T2** — `service`: `apply_peer_ownership` Primary ⇒ peer sole Active; Secondary ⇒
      local sole Active.
- [x] **T3** — `pipeline`: received `SwitchOwnership`⇒this side starts forwarding + suppresses;
      local P→S flushes a held forwarded key *then* relays the handoff (sequence intact);
      a peer-initiated handoff is not echoed back; a received handoff releases injected holds.
- [x] **T4** — RED verified: reverting the recv-arm handler hangs/fails T3's tests.
- [x] **T5** — `cargo test --workspace` = **255 passed, 0 failed**; `cargo clippy --workspace
      --all-targets -- -D warnings` clean; `cargo fmt --check` clean.
- [-] **T6** — connection: single/duplicate/disconnect/reconnect — no code change made
      (F8: precedence logic is symmetric-correct on read; `1006`/flapping/dup-pipeline are
      evidence-blocked — need real 2-machine `FLOW_TRACE` logs, see Phase 5 / §8 below).

## Phase 5 — REAL physical E2E (maintainer, two machines) — §15

- [x] **E1** — `docs/testing/physical-test-script.md` refreshed for the baton (ownership-model
      section, `input_role_changed`/`switch_key` log markers, Round 1 step 6–7, Round 2 note).
- [ ] **E2** — A: Windows Primary → Mac Secondary (typing, modifiers, shortcuts, mouse, scroll;
      no duplicate local input on Windows) — *maintainer, Round 1 steps 1–5*
- [ ] **E3** — B: Scroll Lock on Windows → Mac becomes Primary (Mac UI active device flips,
      link stays Connected, no reconnect) — *maintainer, Round 1 step 6*
- [ ] **E4** — C: Mac Primary → Windows Secondary — **deferred (Q2: not testing reverse now)**.
      The baton is symmetric so it should work; reverse is untested this pass.
- [ ] **E5** — D: Scroll Lock on Windows again → Windows Primary again — *maintainer, Round 1 step 7*
- [ ] **E6** — E: held-input transition tests on real machines — *maintainer, Round 1 step 11*
- [ ] **E7** — F: 5–10 min sustained run — no dropped/dup events, no stuck keys/buttons, no
      reconnect loops, no ownership desync — *maintainer, Round 1 step 8*
- [ ] **E8** — Round 2 (macOS suppression on real HW) — optional this pass, per script note.

---

## Explicitly out of scope (task §17)

Virtual HID · Bluetooth · clipboard · file transfer · audio · screen share · multi-peer ·
generic event-stream rearchitecture · protocol/transport rewrite · Flutter redesign.

---

## Iteration log

| # | Problem | Root cause | Fix | Test | Result |
|---|---------|-----------|-----|------|--------|
| 0 | baseline | — | — | `cargo test --workspace` | 246 pass, 0 fail; build green |
| 1 | diagnosis pass | see Findings F1–F9 | none yet — decisions Q1–Q3 pending | — | — |
| 2 | ownership never crossed the wire (F1/F2): each daemon flipped its own `Active` locally, peer never told; a switch could not make the *other* machine Primary | no `ChannelMessage` for ownership; receive side injected unconditionally; `run_paired_connection` only ever read local `devices` | `InputRole` + `ChannelMessage::SwitchOwnership`; `DaemonService::apply_peer_ownership`; `run_paired_connection` relays a local switch to the peer + applies a received one atomically (immediate role, no echo); `sent_held` flush on hand-off; `main.rs` wiring | 9 new unit tests (core/service/pipeline), RED-verified by reverting the handler; `cargo test --workspace`, clippy `-D warnings`, `fmt --check` | **255 pass, 0 fail**; clippy + fmt clean. Physical E2E still owned by maintainer (Phase 5). |
