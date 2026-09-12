# todos — Complete Flow V1 Architecture and Make It Work E2E

Branch: `main` (was `fix/windows-mouse-forward-freeze`, merged as PR #25 before this pass
started). Baseline: `cargo test --workspace` = **257 passed, 0 failed** on
`x86_64-pc-windows-msvc` before any change in this pass.

Status keys: `[ ]` pending · `[~]` in progress · `[x]` completed · `[-]` blocked/skipped

---

## 0. Context — this is not a green-field task

`main` already carries the *entire* previous V1 pass (`product-first-v1`, merged via PR #24,
plus a physical-testing bugfix in PR #25). See `todos-finish-flow-v1.md` and
`todos-fix-physical-input-switching.md` for that work's own root-cause reports — they are
still accurate and are not repeated verbatim here. Summary of what already exists on `main`:

- Explicit `InputRole { Primary, Secondary }` + `ChannelMessage::SwitchOwnership { sender_role }`
  propagated over the *existing* persistent connection (no reconnect).
- `DaemonService::apply_peer_ownership` applies a peer's ownership change idempotently
  (no-op if already in that state).
- `pipeline::run_paired_connection` is the one authoritative place routing/suppression/
  forwarding derive from (`forwarding` bool, updated on both a local switch and a received
  `SwitchOwnership`).
- Scroll Lock is Primary/Secondary-aware only insofar as `hotkey::runner::spawn_pipeline_switch_filter`
  strips it from the forwarded stream and calls `switch_active_device_local()` — see Finding
  R1 below for the gap this pass closes.
- Held-input reconciliation on ownership transition (`HeldInputTracker`, `sent_held` flush).
- Real Windows suppression (`SuppressionGate`, press/release symmetry) and real macOS
  suppression (`CGEventTap` NULL-return + ported `SuppressionGate`), both self-injection-guarded.
- A **real two-machine physical test round already happened** (Windows ↔ Mac): it found and
  fixed a Windows mouse-forwarding freeze (`5fe746f`, PR #25) via a real `WH_MOUSE_LL` harness
  and `SetCursorPos` recentering. Windows-Primary → Mac-Secondary keyboard+mouse forwarding is
  confirmed working on real hardware. The full round-trip (→ Mac Primary → back to Windows
  Primary) has **not yet** been re-run since the freeze fix landed.

## 1. Fresh root-cause pass against *this* task's spec (verified against current `main`, not assumed)

| # | Area | Finding |
|---|---|---|
| R1 | **Scroll Lock authority** | `spawn_pipeline_switch_filter` (and the standalone `hotkey::runner::spawn`) calls `switch_active_device_local()` on *any* Scroll Lock match, regardless of whether this daemon is currently Primary or Secondary. This is a **documented, deliberate** deviation from a stricter reading of the task (`todos-fix-physical-input-switching.md` §I3b: "either machine's switch key hands the baton... Strict Primary-only is a one-line follow-up gate."). This task's spec (§3, §5, §6, §13) requires strict Primary-only authority. **Fixed this pass** — see §3 below. |
| R2 | **No generation / idempotency guard on the wire** | `ChannelMessage::SwitchOwnership` carries only `sender_role`; idempotency today relies solely on `DaemonService::apply_peer_ownership`'s "already active ⇒ no-op" check, which protects against a *duplicate* application but not a genuinely *stale/reordered* one (task §5's explicit ask). **Fixed this pass** — added a monotonic `generation: u64` field + `OwnershipHandle::try_advance_generation`. |
| R3 | **`local_role()` / `is_local_primary()` / etc. don't exist** | The devices list's `Active` flag is *not* a reliable proxy for "am I Secondary": a fresh/never-switched daemon and a daemon that was just told `Secondary` by its peer both leave `Active = self` in that daemon's own device list (see docs/architecture note added this pass). Task §3 asks for explicit accessors. **Fixed this pass** — new `OwnershipHandle` (daemon/src/ownership.rs), exposed off `DaemonService`. |
| R4 | **Validation on receipt** | Re-read against task §5's four bullets: (a) "primary belongs to the paired session" — structurally guaranteed already, since the receiving pipeline gets `peer_id` from the *connection's own* Noise-authenticated identity, never from the message body (better than the task's literal `primary_device_id` suggestion — kept this design, documented why). (b) "sender authorized" — enforced at the *source*: only a daemon that itself gated as Primary ever emits `SwitchOwnership` (R1's fix), so a peer cannot be un-authorized and still have sent one over this single trusted P2P connection. (c) "malformed messages rejected" — already true, `serde` deserialization failure ⇒ `Err(_) ⇒ break 'conn` in the recv match. (d) "stale can't revert" — R2's generation guard. No further code needed for (a)/(c); documented. |
| R5 | **Connection lifecycle / arbitration** | Unchanged from `todos-fix-physical-input-switching.md` §8 and `todos-finish-flow-v1.md` F8: precedence logic (`connection_precedence`, smaller-identity-key wins) looks correct on read, `claim_lost` is the designed outcome for the losing direction. Still evidence-blocked pending `FLOW_TRACE` logs from a real simultaneous two-machine start — no code smell found this pass to justify a speculative change. Out of scope for this pass (no reproduction, no regression found). |
| R6 | **macOS suppression** | Per `docs/architecture/implementation-map.md`, code-complete + unit-tested, previously "unverified on hardware." Given PR #25's commit message, the maintainer has since run real Windows↔Mac sessions (Mac received forwarded input) — suppression must be at least partially exercised, but the round-trip (Mac-as-Primary suppressing *its own* input) is still not confirmed in any doc. Flagged for the physical checklist below (§7). |
| R7 | **Observability** | Existing `hop_note!`/`hop!` lines (`pipeline_gate_init`, `input_role_changed`, `switch_key`, `switch_consumed`, `send_gate`, `frame_sent`, `frame_recv`, `injected`, `replay_drop`) already cover this task's §12 intent under different (pre-existing, equally clear) names. Added this pass: a `stage="ownership_rejected"` line for a stale/duplicate generation, and a `stage="switch_ignored"` line for a Secondary-side Scroll Lock press. No wholesale rename — would touch every existing test and log consumer for no behavioral gain. |

## 2. Decisions (V1 policy, per task §4)

- **Initial ownership policy**: deterministic, in-memory, per-process — a daemon with no live
  peer pipeline (or one that just started) is always `Primary` of itself by default
  (`OwnershipHandle::default()`). Not persisted to disk (task §4: "do not persist stale
  ownership indefinitely"). On disconnect, role resets to `Primary` (§3 I3 below) so a machine
  that was Secondary when its peer vanished isn't permanently locked out of initiating on the
  next connection.
- **Authority scope**: only Scroll Lock (the switch key) is gated to Primary-only this pass.
  The IPC `switch_active_device` path (Flutter UI device picker) is **not** gated — task §5's
  concrete test list (§13) is scoped to Scroll Lock; gating the UI path too is a bigger product
  question (can a Secondary's own UI request control back?) left open, documented as a known
  limitation rather than guessed at.
- **Reverse-direction physical validation**: requires Mac's own physical Scroll Lock press
  (task §6's explicit fallback question). The maintainer has a MacBook with its own keyboard
  (confirmed by the PR #25 commit message describing a real two-machine session), so this is
  the mechanism — not a documented gap, an actual plan. See §7.

## 3. Implementation (this pass)

- [x] **I1** — `daemon/src/ownership.rs` (new): `OwnershipHandle` — `Arc<AtomicBool>` role +
      `Arc<AtomicU64>` generation, cheap `Clone`, `Send + Sync`. `role()`, `is_primary()`,
      `is_secondary()`, `peer_is_primary()`, `set_role()`, `bump_generation()`,
      `try_advance_generation(u64) -> bool` (strictly-greater CAS loop). Unit-tested in
      isolation (no `DaemonService`, no tokio runtime needed).
- [x] **I2** — `core/src/channel/mod.rs`: `ChannelMessage::SwitchOwnership` gains
      `generation: u64`. Round-trip test updated.
- [x] **I3** — `daemon/src/pipeline/mod.rs::run_paired_connection`: new `ownership:
      OwnershipHandle` parameter.
      - Local-flip branch: `ownership.bump_generation()` before building the outgoing
        message; `ownership.set_role(new_role)` alongside the existing `forwarding =
        now_forwarding`.
      - Peer-received branch: `ownership.try_advance_generation(generation)` gates the whole
        branch — `false` ⇒ log `stage="ownership_rejected" reason="stale_or_duplicate_generation"`
        and `continue` (nothing else touched: no forwarding change, no suppression call, no
        `on_peer_ownership`); `true` ⇒ proceed as before, plus `ownership.set_role(my_role)`.
      - End of function (after the `'conn` loop, both disconnect paths): `ownership.set_role(InputRole::Primary)`
        — never leave a daemon permanently Secondary once its peer connection is gone.
- [x] **I4** — `daemon/src/service/mod.rs`: `DaemonService` gains an `ownership: OwnershipHandle`
      field (created in `from_state`, so every constructor gets one) + accessors
      `ownership_handle()`, `local_role()`, `is_local_primary()`, `is_local_secondary()`,
      `peer_is_primary()`. `apply_peer_ownership`'s signature is unchanged — role/generation are
      now settled inside the pipeline *before* it's called, so it stays focused on the devices
      list.
- [x] **I5** — `daemon/src/hotkey/runner.rs::spawn_pipeline_switch_filter`: gates the switch on
      `service.is_local_primary()`. Primary ⇒ unchanged behavior (consume + switch). Secondary
      ⇒ still consumes the key (never forwarded, never leaks to the peer as a normal
      `InputEvent` — task §6), logs `stage="switch_ignored" reason="not_primary"`, does **not**
      call `switch_active_device_local()`. The standalone `hotkey::runner::spawn` needed no
      change: it already stands down entirely via `peer_pipeline_active()` while a pipeline
      owns authority, and outside a pipeline there's no peer to switch to, so the default
      `Primary` role is always correct there.
- [x] **I6** — `daemon/src/main.rs::run_peer_pipeline`: passes `service.ownership_handle()`
      into `run_paired_connection`.

## 4. Tests (this pass)

- [x] **T1** — `ownership.rs`: defaults Primary/generation 0; `set_role` flips both directions;
      `peer_is_primary` mirrors `is_secondary`; `bump_generation` increments from 1;
      `try_advance_generation` accepts strictly-greater, rejects an exact duplicate and a
      lower (stale) value.
- [x] **T2** — `pipeline::tests`: all 9 existing `run_paired_connection` call sites updated for
      the new `ownership` parameter (each test gets its own fresh `OwnershipHandle::new()`
      unless the test itself asserts on role, per T3); the two tests that hand-construct a
      `ChannelMessage::SwitchOwnership` now set `generation: 1`.
- [x] **T3** — `pipeline::tests` (new): `a_stale_or_duplicate_ownership_generation_is_ignored`
      — accept generation 5, then resend generation 3 (stale) and confirm no second suppress
      call, no re-injection, state unchanged.
- [x] **T4** — `pipeline::tests` (new): `run_paired_connection_resets_role_to_primary_on_disconnect`
      — drive this side Secondary via a received handoff, close the connection, confirm the
      `OwnershipHandle` passed in reads `Primary` again afterward.
- [x] **T5** — `hotkey::runner::tests` (new):
      `a_switch_key_press_while_secondary_is_consumed_but_does_not_switch` — set the test
      service's `ownership_handle()` to `Secondary` before running the filter; Scroll Lock
      down+up must not reach the output stream *and* must not change the active device.
- [x] **T6** — `cargo test --workspace` green after the change (see §6 for the actual run).

## 5. Explicitly NOT done this pass (and why)

- [-] **Generic multi-path authorization / crypto-signed ownership messages** — V1 has exactly
      one trusted, Noise-authenticated peer connection; the source-side gate (I5) plus the
      generation guard (I3) already make an unauthorized or stale switch structurally
      impossible without a second protocol or transport rewrite, both explicitly out of scope
      (task §17).
- [-] **Gating the IPC `switch_active_device` path** — see §2 "Authority scope" above; left as
      existing (ungated) behavior, documented as a known limitation, not silently changed.
- [-] **Connection-lifecycle rewrite (#4/#5 from the older trackers)** — still evidence-blocked;
      no reproduction found this pass; not touched.

## 6. Verification run (this pass)

- [x] `cargo test --workspace` — before: 257 passed, 0 failed. After: **267 passed, 0 failed**
      (10 new: 7 `ownership.rs` unit tests, 2 new `pipeline::tests`, 1 new
      `hotkey::runner::tests`). Confirmed clean across 3 consecutive full runs.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` — clean. `run_paired_connection`
      crossed clippy's default 7-argument limit at 8 (adding `ownership`); silenced with
      `#[allow(clippy::too_many_arguments)]` + a comment, rather than a struct-bundling
      refactor of every call site (including this module's own 9 tests) for a lint threshold.
- [x] `cargo fmt --check` — clean on every file this pass touched. `platform/src/{linux,macos}/translate.rs`
      fail `fmt --check` on this machine's rustfmt version — pre-existing drift, confirmed by
      reverting `cargo fmt`'s changes to those two untouched files; not part of this pass.
- [x] **Found and fixed a real, pre-existing latent race** while chasing an intermittent hang
      in `cargo test --workspace` (reproduced 3 times, always on the same test, always a true
      stall — not slowness: the stuck process accumulated ~0s of further CPU across minutes of
      wall-clock time). Root cause: `pipeline::tests::a_peer_initiated_handoff_is_not_echoed_back`
      sent a `SwitchOwnership` handoff, then immediately sent a capture event on an unbounded
      channel with **no synchronization point** — unlike every sibling test in the same file,
      which all synchronize on a real `suppress_local` channel before touching `capture_tx`.
      Both messages could become "ready" before the pipeline task was first polled; `tokio::
      select!` has no fixed priority among ready branches, so it could poll the capture-event
      branch first, while `forwarding` was still `false` — silently dropping the event (by
      design: "captured while inactive is dropped, not queued") — and the test's `peer_side.
      recv().await` then waited forever for a frame that was never going to arrive. This bug
      predates this pass (the same race was always latent); editing this exact test to add the
      new `generation` field is what put it back in front of us. Fixed by giving it the same
      `suppress_local`-channel synchronization pattern every other test in the file already
      uses — not a change to `run_paired_connection` itself. Re-ran the full workspace suite 3
      more times after the fix: clean every time.

## 7. Real physical E2E — maintainer, two real machines (not performed by the agent)

Carried over from `todos-finish-flow-v1.md` Phase 5, re-scoped now that Primary-only gating
exists and the mouse-freeze fix (PR #25) is in:

- [ ] **E1** — Windows Primary → Scroll Lock (Windows) → Mac Primary. Confirm: Windows stops
      receiving local keyboard/mouse; Mac receives it; no reconnect; link stays Connected.
- [ ] **E2** — **New this pass**: with Windows now Secondary, press Scroll Lock **on Windows
      again** and confirm it is *ignored* (no switch, nothing typed on Mac, nothing switched
      locally) — proves R1's gate is real on hardware, not just in unit tests.
- [ ] **E3** — Mac Primary → Scroll Lock **on the Mac's own keyboard** → Windows Primary again.
      This is the task §6 "document the exact mechanism for the reverse transition" — the
      mechanism is: Mac's own physical Scroll Lock, gated the same way Windows' is, since Mac
      is now `Primary` per this pass's symmetric implementation.
- [ ] **E4** — Held-input safety across a real switch: hold a modifier (Shift/Ctrl/Alt) across
      Scroll Lock in both directions; confirm no stuck key, no duplicate release.
- [ ] **E5** — Mouse: move, left/right/middle click, scroll, in both directions post-freeze-fix.
- [ ] **E6** — 5–10 minute sustained mixed-input session in each direction; no drift, no
      reconnect, no ownership desync.
- [ ] **E7** — Rapid double-press of Scroll Lock (debounce + generation guard together) on
      whichever machine is currently Primary; confirm exactly one switch, not zero or two.

None of E1–E7 can be performed from this environment (single Windows host, no Mac, and an
unattended suppressing hook risks locking out the operator). Code, unit/integration tests, and
this checklist are what this pass can deliver; the maintainer runs the checklist itself.

---

## Explicitly out of scope (task §17, unchanged)

Virtual HID · Bluetooth · clipboard · file transfer · audio · screen share · multi-peer ·
generic event-stream rearchitecture · protocol/transport rewrite · Flutter redesign.
