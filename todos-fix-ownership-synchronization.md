# todos — Fix Flow V1 Ownership Synchronization (split-brain)

Branch: `main` (base commit `6ba9dac`, which already carries the "Complete Flow
V1" pass — `todos-complete-flow-v1.md` — 267 tests green). This pass validates
and fixes the incoming code-review prompt's core claim: **split-brain
ownership on disconnect/reconnect.**

Status keys: `[ ]` pending · `[~]` in progress · `[x]` completed · `[-]` cancelled/not applicable

---

## 0. Validation (before writing any fix)

The incoming prompt's diagnosis was checked against `main` at `6ba9dac`, not
assumed. Confirmed **real and current**, not hypothetical:

- [x] **`daemon/src/pipeline/mod.rs:515`** (`run_paired_connection`'s teardown,
      both disconnect paths): `ownership.set_role(InputRole::Primary)`
      unconditionally, regardless of what role this side actually held. Since
      *both* peers run this same function and both experience the disconnect,
      both independently reset to `Primary` — the exact split-brain the prompt
      describes. There is even a test
      (`run_paired_connection_resets_role_to_primary_on_disconnect`) that
      asserts this as *correct* behavior — it isn't; it's the bug, now
      codified.
- [x] **`sender_role.opposite()`** (`pipeline/mod.rs:465`) is exactly the
      forbidden inference pattern the prompt calls out: the receiver never
      sees an absolute owner, only "flip whatever I currently think".
- [x] No `session_id`/`epoch` concept exists anywhere in `core/` — confirmed
      via grep. Only a per-process, never-persisted `generation: u64`
      (`daemon/src/ownership.rs`), which is *not* reset across a reconnect
      (good) but also never used to reconcile two peers' beliefs when they
      *do* reconnect (the actual gap).
- [x] Connection direction (dialer vs. acceptor) is already independent of
      ownership — `connection_precedence` only arbitrates which of two
      simultaneous *connections* survives, never who is Primary. No change
      needed there.
- [x] The standalone hotkey runner (`hotkey::runner::spawn`) already has no
      ownership dependency and is not gated by `is_local_primary()` — it only
      stands down while a peer pipeline is active
      (`peer_pipeline_active()`). This means the forced Primary-reset in (1)
      was never actually needed to avoid a "stuck Secondary" — the standalone
      runner already provides a way back once a connection ends. Confirmed by
      reading, not assumed.

## 1. Root cause

`run_paired_connection` treats "this connection ended" as "resolve ownership
back to a safe default" (`Primary`), instead of leaving the last-known state
alone and reconciling it explicitly the next time a connection to this peer
is established. Two peers whose connection drops therefore diverge
immediately and have no mechanism to converge again on reconnect — reconnect
just re-derives `forwarding` from each side's own local `devices` snapshot
(also independent per side), so nothing before this pass ever brought the two
peers back into agreement.

## 2. Fix design

- **Wire protocol** (`core/src/channel/mod.rs`): replace
  `ChannelMessage::SwitchOwnership { sender_role, generation }` with
  `ChannelMessage::OwnershipChanged { primary_device_id: DeviceId, generation: u64 }`
  — absolute, not a role a receiver has to invert. A receiver validates
  `primary_device_id` is one of the two devices it actually knows (itself or
  its peer); anything else is rejected and logged
  (`reason="invalid_target_owner"`).
- **One message serves both purposes** — a live mid-session handoff *and* the
  new reconnect handshake below — rather than adding a second message type,
  per the task's "channel/protocol changes only as needed."
- **`daemon/src/ownership.rs`**: add `generation_now()` (plain getter) and
  `reconcile(resolved_primary_is_local: bool, resolved_generation: u64) -> bool`
  (sets role unconditionally to the resolved value; raises the generation
  floor to `max(current, resolved)`, never lowers it). `try_advance_generation`
  is unchanged and still gates a live in-session handoff.
- **`daemon/src/pipeline/mod.rs::run_paired_connection`**: at the very start of
  the connection, before touching `devices`/`forwarding`/suppression, both
  sides exchange one `OwnershipChanged` each carrying their own current
  belief, then both independently run the same pure, deterministic
  `resolve_ownership` function:
  - strictly higher `generation` wins outright (it is fresher);
  - an exact tie where both already agree is a no-op;
  - an exact tie where they *disagree* (including "first-ever connection,
    generation 0 vs 0, both default-Primary of themselves") is broken by
    comparing device ids — the lexicographically smaller id is Primary. Both
    sides compare the same two ids, so both always reach the same verdict,
    independent of who dialed and who accepted.
  - This one function is what makes "deterministic initial owner" and
    "deterministic reconnect reconciliation" the *same* mechanism rather than
    two separate policies.
- **Teardown**: delete the unconditional `ownership.set_role(InputRole::Primary)`.
  Role is left exactly as it was; the next connection's handshake reconciles
  it, rather than every disconnect blindly guessing "Primary."
- **Authorization**: unchanged from the prior pass — enforced at the *source*
  (only a daemon that itself gated as Primary ever emits a live handoff,
  `hotkey::runner::spawn_pipeline_switch_filter`'s existing
  `is_local_primary()` check) plus the new receiver-side "is this a known
  device" validation above. The IPC `switch_active_device` path remains
  intentionally ungated, carried over from `todos-complete-flow-v1.md`'s own
  documented scope decision (still an open product question, not silently
  changed here) — see §5.

## 3. Implementation

- [x] **I1** — `core/src/channel/mod.rs`: `SwitchOwnership` → `OwnershipChanged { primary_device_id, generation }`. Updated round-trip test.
- [x] **I2** — `daemon/src/ownership.rs`: add `generation_now()`, `reconcile()`. Unit tests for both.
- [x] **I3** — `daemon/src/pipeline/mod.rs`: add `resolve_ownership()` (pure, unit-tested directly) and the connection-opening handshake in `run_paired_connection`; rewrite the local-flip send and peer-receive branches around `OwnershipChanged`/`primary_device_id`; delete the disconnect reset.
- [x] **I4** — Update every existing `pipeline::tests` call site for the new handshake (peer side must answer the opening exchange before the rest of each test's scenario) and for the renamed message.
- [x] **I5** — Replace `run_paired_connection_resets_role_to_primary_on_disconnect` with `run_paired_connection_does_not_reset_role_on_disconnect` (the direct regression guard for the fix).
- [x] **I6** — New test: a simulated reconnect (second `run_paired_connection` reusing the same `OwnershipHandle`) after a disconnect converges on the peer's persisted belief instead of both sides claiming Primary.
- [x] **I7** — New test: an `OwnershipChanged` naming a third, unknown device id is rejected (`invalid_target_owner`), no state change.
- [x] **I8** — New unit tests on `resolve_ownership` directly: higher generation wins regardless of call-argument order (direction-independence); tie + agreement is a no-op; tie + disagreement breaks deterministically and identically from either side's perspective.

## 4. Explicitly NOT done this pass (and why)

- [-] **Gating the IPC `switch_active_device` path** — carried over unchanged
  from `todos-complete-flow-v1.md` §2: whether a Secondary's own UI may
  request control back is a genuine open product question, not something to
  guess at here. Still documented, not silently changed.
- [-] **A separate `session_id`/`epoch` wire field** — the reconciled
  `generation` (now never reset across a reconnect, and now actually used to
  arbitrate a reconnect) already serves the "stale message from a previous
  session can't win" property the task asks `epoch` for for this exact
  two-device, single-trusted-connection channel model, where a message can
  only ever be `recv`'d through the specific `Channel` object the connection
  that received it owns — there is no queue or buffer a stale connection's
  message could replay into a new one through. Adding a second counter with
  the same job would be an unjustified wire/protocol change (task §10: "only
  as needed").
- [-] **Persisted ownership record surviving a process restart** — task §4
  says "do not persist stale ownership indefinitely"; V1's in-memory,
  per-process default (`OwnershipHandle::default() = Primary`) combined with
  the reconnect handshake already makes a fresh process converge correctly
  the moment it reconnects, without needing disk state.
- [-] **Broad channel/transport rewrite** — not touched; the fix is entirely
  in the ownership layer and one wire message.

## 5. Verification run

- [x] `cargo fmt --all -- --check` — clean on every file this pass touched.
      `platform/src/{linux,macos}/translate.rs` still fail on this machine's
      rustfmt version — pre-existing drift from before this pass (confirmed:
      those two files were never edited here), same as
      `todos-complete-flow-v1.md` §6 already noted.
- [x] `cargo check --workspace` — clean.
- [x] `cargo test --workspace` — **277 passed, 0 failed** (baseline going into
      this pass was 267; net +10: 5 new `ownership.rs` unit tests
      [`generation_now`, `reconcile` × 3, a stale-handoff-after-reconcile
      guard], 4 new `resolve_ownership`/handshake unit+integration tests in
      `pipeline.rs`, 1 renamed regression test — offset by removing the one
      test that asserted the old, wrong reset-to-Primary behavior).
- [x] `cargo clippy --workspace --all-targets -- -D warnings` — clean.

## 6. Docs updated for consistency

`docs/architecture/implementation-map.md`, `docs/testing/physical-test-script.md`,
and `daemon/README.md` all named the old `SwitchOwnership`/`sender_role.opposite()`
mechanism directly — updated to describe `OwnershipChanged`/`primary_device_id`
and the new reconnect handshake, plus a new physical-test-script step (10b)
specifically targeting the split-brain scenario this pass fixes.

## 7. Real physical E2E — not performed by the agent

Same caveat as `todos-complete-flow-v1.md` §7: single Windows host, no Mac in
this environment. The maintainer should re-run that file's E1–E7 checklist
after this pass, plus the new **step 10b** added to
`docs/testing/physical-test-script.md`: kill the network link mid-session
(not just Scroll Lock) while the Mac is Primary, restore it, and confirm
Windows does not silently become Primary too, and both machines agree on
`resolved_primary` in their logs once reconnected.
