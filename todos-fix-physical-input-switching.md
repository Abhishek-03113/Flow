# Fix Flow Physical Device Input Switching

Branch: `fix/windows-local-suppression` (off `main`).
Scope agreed with maintainer: **Critical Fix #1 (Windows local suppression) + harden #2/#3**,
and the hook-ordering blocker (§5) resolved via **Option A** (peer pipeline owns switch-key
authority while it runs).
Fixes #4 (connection ownership) and #5 (WebSocket 1006) are **evidence-blocked** — see the
bottom section — they need `FLOW_TRACE` logs from a real two-machine run, which this
environment cannot produce.

Status of this branch: implemented + unit-tested + `cargo test --workspace` /
`clippy -D warnings` / `fmt` all green, natively on `x86_64-pc-windows-msvc`. **Not**
validated on real hardware or across two machines. Not committed.

---

## 1. Investigation — the complete runtime path

| # | Layer | File | Finding |
|---|---|---|---|
| 1 | Windows keyboard capture | `platform/src/windows/capture.rs` | `WH_KEYBOARD_LL` hook. `keyboard_proc` translated + `sender.send()`, then **always** `CallNextHookEx`. Never returned `LRESULT(1)`. |
| 2 | Windows mouse capture | same | `WH_MOUSE_LL`, same shape. |
| 3 | Active-device / routing state | `daemon/src/service/mod.rs` (`switch_active_device`, `switch_active_device_local`) | Single source of truth: the `devices` map, **exactly one** `DeviceState::Active`, published on a `watch` channel. No ambiguity between "device metadata" and "routing state" — `Active` = input target, `Connected` = paired/reachable, `DaemonLinkState` = link health. Three separate, clean concepts. |
| 4 | Local-suppression decision | `daemon/src/pipeline/mod.rs` `run_paired_connection` | Already correct: calls `suppress_local(bool)` on connect and on every active-device change, keyed on `is_peer_receiving_input` (is *the peer* `Active`?). Releases on disconnect. |
| 5 | Serialization | `pipeline` + `flow_core::channel::ChannelMessage::Input` | Sequence-numbered frames, replay guard. Fine. |
| 6 | Peer connection | `daemon/src/channel/{tcp,noise}.rs` | WebSocket + Noise. Fine. |
| 7 | Remote reception | `pipeline::run_paired_connection` recv arm | Fine; `HeldInputTracker` synthesizes releases on drop. |
| 8 | macOS injection | `platform/src/macos/injector.rs` | Real `CGEventPost`. Unit-tested; **never executed on hardware in this repo.** Not modified. |
| 9 | Switch-key handling | `daemon/src/hotkey/` | `SwitchKeyMatcher` + `SwitchDebouncer` + live rebinding, all unit-tested. Fed from its **own** `DefaultInputCapture` instance (see the blocker below). |
| 10 | Connection arbitration | `daemon/src/main.rs` (`claim_and_run`, `try_claim_peer`), `service/mod.rs` (`connection_precedence`) | `ConnectionPrecedence::{Preferred,Redundant}` computed identically on both ends from the two identity public keys; `try_claim_peer` HashSet dedup. `claim_lost` / "another task already holds this peer's connection slot" is the **designed, correct** outcome for the losing direction, not a bug in itself. |

### Logging reality check (affects the prompt's run recipes)

The daemon did **not** honor `RUST_LOG`. `daemon/src/logging.rs` installed a
`tracing_subscriber` **`LevelFilter` reload layer**, not an `EnvFilter`.

**RESOLVED (product-first V1, iteration 1).** The reload layer is now an `EnvFilter`:
- `RUST_LOG` set → wins verbatim.
- `RUST_LOG` unset → scoped default `flow={level},flow_daemon={level}`.
- `FLOW_TRACE=1` → `flow=trace,flow_daemon=trace` (scoped — no `tokio_tungstenite` frame
  noise), not a global `TRACE` floor.
- `settings.debug_logging` → still toggles `DEBUG` at runtime via IPC (inert no-op if
  `RUST_LOG` was set at startup).

`RUST_LOG=flow=debug` is the documented product-debug command. The lifecycle trail is still
on target `flow::hop` (`hop!` = TRACE, `hop_note!` = DEBUG); product lines
(`[INPUT]/[SWITCH]/[PEER]/[ERROR]`) are on the bare `flow` target via `logging::product::*`.
See `daemon/README.md` "Structured logging".

---

## 2. Root cause(s)

- **#1 Windows suppression** — never implemented. `set_suppress_local` was a hard
  `Err(WindowsCaptureError::SuppressionUnsupported)` stub and the hook procs never
  withheld. The mechanism (return `1` instead of `CallNextHookEx`) and the state path
  (thread-local `STATE`) were both already understood in the code comments; nobody had
  built + tested it. **Fixed.**
- **#2 routing state** — *not a defect.* The `InputRoute` enum the prompt proposes would
  duplicate the already-atomic single-`Active` invariant. No code change; added logging
  only.
- **#3 switch key** — matcher/debounce/rebinding already correct. Two real gaps surfaced,
  both consequences of #1 actually working now (see blocker).
- **#4 / #5** — no root cause established. Needs real-run evidence.

---

## 3. Implementation (this branch)

### Fix #1 — Windows local suppression  ✅ implemented + unit-tested

`platform/src/windows/capture.rs`:

- Removed `WindowsCaptureError::SuppressionUnsupported`.
- `WindowsInputCapture` + `CaptureState` gained a shared `Arc<AtomicBool>` suppress flag
  (shared, not thread-local, because `set_suppress_local` runs on the daemon's pipeline
  task — a different thread than the hook thread).
- `set_suppress_local(bool)` now stores the flag and returns `Ok(())`. Safe before
  `start()` / after `stop()`.
- New `SuppressionGate` (thread-local, in `CaptureState`) decides per callback whether to
  withhold the event from the local OS. Rule = **press/release symmetry**: a `KeyUp` /
  `ButtonUp` is withheld **iff its matching down was** — so a mid-hold suppression toggle
  never strands a half-press locally, and the switch key's own key-up (which lands just
  after the route flips) does not leak a phantom key-down into the local foreground app.
- `keyboard_proc` / `mouse_proc`: still translate + `sender.send()` (peer keeps getting
  input), then consult the gate and `return LRESULT(1)` when it says withhold.
- `guard_hook_body` now returns `bool` (the withhold decision); on a caught panic it
  returns `false` — **fail open**, never trap the user's own keyboard/mouse because of a
  bug in translation or the gate.

Modifiers, mouse move, wheel, buttons, and key/button-up all follow the same rule.

### Harden #2 / #3 — routing logging  ✅

- `daemon/src/pipeline/mod.rs` — the `send_gate` hop now carries
  `route=remote|local  forwarded=<bool>  suppressed=<bool>  kind=<event_kind>`.
- `daemon/src/service/mod.rs` — both switch paths now log
  `stage=switch  from=<id|none>  to=<id>  trigger=ipc|hotkey`.
- `daemon/src/hotkey/runner.rs` — a switch consumed inside a pipeline logs
  `stage=switch_consumed  trigger=hotkey`.

### §5 blocker — Option A  ✅ implemented

- `DaemonService::enter_peer_pipeline()` → RAII `PeerPipelineGuard`; `peer_pipeline_active()`
  reports whether ≥1 pipeline is live (`active_peer_pipelines: Arc<AtomicUsize>`, mirrors
  the existing `IpcClientGuard` pattern).
- `hotkey::runner::spawn_pipeline_switch_filter(service, rx) -> rx` — runs a
  `SwitchKeyMatcher` + `SwitchDebouncer` over the peer pipeline's *own* capture stream
  (which still carries the switch key even while the OS hook withholds it), calls
  `switch_active_device_local()` on a match, and returns the stream **minus** the switch
  key's `KeyDown` *and* its matching `KeyUp` (tracked in a `consumed_keys` set) so the
  switch key is never forwarded to the peer. Live rebinding via the settings watch.
- `hotkey::runner::spawn` (standalone) — now skips its own switch while
  `service.peer_pipeline_active()`, so a press isn't handled twice.
- `main.rs::run_peer_pipeline` — holds `enter_peer_pipeline()` for the connection's life
  and feeds `run_paired_connection` the *filtered* stream.

Known limitation of the combo case: for a multi-key binding (e.g. `Ctrl+Shift+Space`) only
the final key and its release are stripped; the modifier down/up events still reach the
peer. Harmless (bare Ctrl/Shift do nothing) but noted. The default `ScrollLock` binding is
fully clean.

---

## 4. Tests

### Added (`platform/src/windows/capture.rs`, `mod tests`) — 10, all passing

- `with_suppression_off_no_keyboard_event_is_withheld`
- `with_suppression_on_a_full_press_and_release_are_both_withheld`
- `a_syskey_press_and_release_are_treated_like_a_plain_one`
- `a_release_is_withheld_after_suppression_is_handed_back_if_its_press_was_withheld`
- `a_release_reaches_the_local_app_when_its_press_did_even_if_suppression_turned_on_since`
- `the_switch_key_own_press_that_completes_before_suppression_is_a_clean_local_press`
- `with_suppression_on_mouse_movement_and_wheel_are_withheld`
- `with_suppression_off_mouse_movement_and_wheel_pass_through`
- `a_mouse_button_release_is_withheld_iff_its_press_was`
- `withheld_buttons_are_tracked_independently_per_button`

TDD: stubbed the gate → watched 6 fail for the right reason → implemented → 10/10 green.

### Added (`daemon/src/hotkey/runner.rs`, `mod tests`) — 3, all passing

- `a_non_switch_event_passes_straight_through`
- `the_switch_key_is_consumed_not_forwarded_and_triggers_a_switch`
- `a_rebind_makes_the_old_switch_key_forward_normally_again`

TDD: passthrough stub → watched the switch test fail → implemented → 3/3 green.

### Added (`daemon/src/service/mod.rs`, `mod tests`) — 1

- `peer_pipeline_guard_tracks_a_nested_count`

### Full sweep — green

`cargo test --workspace` (162 daemon + 30 platform + the rest),
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`. Ran natively
on `x86_64-pc-windows-msvc` (this host).

### Not added — needs real hardware

An end-to-end two-daemon integration test that actually exercises suppression + switch across
a socket. The `FLOW_TEST_HOOKS` / `debug_inject_input` path can drive synthetic events
through a real pipeline (see `daemon/examples/drive_two_daemons.rs`), but it cannot observe
OS-level suppression — that half only exists once real hooks run on a real Windows desktop.

---

## 5. RESOLVED (Option A) — hook ordering starved the switch-key matcher while suppressing

> Fixed on this branch. See "§5 blocker — Option A" under section 3 for what was built.
> The analysis below is kept for the record.

**The daemon installs two independent `WH_KEYBOARD_LL`/`WH_MOUSE_LL` hook pairs:**

1. `hotkey::runner::spawn` (at startup) — its own `DefaultInputCapture`, feeds the
   `SwitchKeyMatcher`. Never suppresses.
2. `run_peer_pipeline` (per connection) — a second `DefaultInputCapture`, feeds
   `run_paired_connection`. **This is the one that gets `set_suppress_local`.**

Windows calls LL hooks **most-recently-installed first**, and a hook that returns
`LRESULT(1)` without calling `CallNextHookEx` **ends the chain** — earlier hooks in the
same process are skipped. #2 installs after #1, so when #2 withholds an event, **#1 never
sees it**. Result once suppression is active:

- the switch-key matcher (fed by #1) receives nothing → **you cannot press the switch key
  to hand control back**;
- worse, `run_paired_connection` (fed by #2) forwards *everything* while `forwarding`, so
  the switch key is **typed into the remote machine** instead of switching.

This was not hypothetical — the moment `set_suppress_local` started really suppressing (via
the `run_peer_pipeline` call that already existed), this would have regressed switching.
**Option A below was chosen and implemented.**

### Options considered

- **A — pipeline owns switch authority while active (chosen, implemented).**
  In `run_peer_pipeline`, run a `SwitchKeyMatcher` + `SwitchDebouncer` over the *same*
  capture-bridge stream (it's forwarded to the channel *before* the OS chain is broken, so
  the matcher still sees the switch key while suppressed). Filter matched switch events out
  of the stream handed to `run_paired_connection` so they are never forwarded to the peer.
  Add an `AtomicUsize` "active peer pipelines" count on `DaemonService`; the standalone
  `hotkey::runner` yields switch authority (skips its own `switch_active_device_local`)
  while that count is non-zero.
- **B — one shared capture instance for the whole daemon (cleanest, bigger).**
  `main.rs` owns a single `DefaultInputCapture`; fan its event stream out (broadcast) to
  the matcher and to every peer pipeline. Suppression on that one instance. Removes the
  dual-hook problem entirely but changes `run_paired_connection`'s input type and ripples
  through the pipeline tests.
- **C — ship the primitive un-activated.** Keep this branch's tested gate but guard the
  `set_suppress_local` call behind a not-yet-default flag until A or B lands. Lowest risk,
  no user-visible progress.

---

## 6. Physical validation

**Not performed. Cannot be, here.** No macOS hardware, and running an all-input-withholding
`WH_KEYBOARD_LL` hook unattended on the dev machine risks locking the operator out of their
own keyboard/mouse if anything is wrong.

### When A or B is done, the maintainer must run, on two physical machines:

Windows daemon (PowerShell):
```powershell
$env:FLOW_ENV="development"; $env:FLOW_DEV="1"; $env:FLOW_SECURITY="insecure"; $env:FLOW_TRACE="1"
cargo run -p flow-daemon
```
Mac daemon:
```bash
FLOW_ENV=development FLOW_DEV=1 FLOW_SECURITY=insecure FLOW_TRACE=1 cargo run -p flow-daemon
```
Mac Flutter:
```bash
cd flutter && flutter run -d macos --dart-define=FLOW_ENV=development --dart-define=FLOW_DAEMON_MODE=ipc
```
(`FLOW_TRACE=1`, not `RUST_LOG` — see §1. Expect `tokio_tungstenite` frame noise until the
`EnvFilter` follow-up lands.)

Acceptance checklist (unverified):
- [ ] Mac selected → Windows stops receiving normal keyboard/mouse; Mac receives it.
- [ ] Switch key → Windows active; Mac stops receiving; Windows receives normally.
- [ ] Switch key itself never types on Mac and never leaks to the Windows foreground app.
- [ ] Key-up / button-up never lost across a switch (the `SuppressionGate` symmetry).
- [ ] Repeated switching stays stable.
- [ ] Disconnect Mac → Windows input immediately usable again (suppression released).
- [ ] Reconnect Mac → streaming resumes; switching works again.

---

## 7. Reverse direction (Mac → Windows suppression)

Out of scope (no Mac hardware). The `InputCapture::set_suppress_local` trait contract and
the pipeline wiring are already symmetric, so a `MacosInputCapture` implementation
(active `CGEventTap` returning null + `kCGEventTapDisabledByTimeout` re-arm) drops in
later without pipeline changes. `MacosCaptureError::SuppressionUnsupported` still returned
today.

---

## 8. #4 / #5 — evidence-blocked

- **#4 connection ownership.** The precedence + claim design looks sound on read. Whether
  the *winning* pipeline ever gets torn down by the loser's `close()`, and whether the
  Flutter IPC layer mis-reads that intentional peer-socket close as a fatal daemon
  disconnect, can only be judged from `FLOW_TRACE=1` logs of two real daemons starting
  simultaneously. Capture: `stage=claim`, `claim_lost`, `claim_dropped`, `pipeline_up`,
  `pipeline_down`, `link_connected` lines from both sides.
- **#5 WebSocket 1006.** No 1006 handling exists in the daemon. Need a real capture showing
  *which* socket closed abnormally (IPC `127.0.0.1:47823` vs peer channel) and the
  preceding lines. Until then any change is a guess.

Provide those logs and these become actionable.
