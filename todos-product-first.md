# todos — product-first V1

Goal: the smallest Flow vision working on two physical computers —
**one keyboard + one mouse → Windows and Mac, Scroll Lock switches control.**

Branch: `product-first-v1`. Baseline (39856dc): `cargo build` / `clippy -D warnings` /
`fmt --check` green, `cargo test --workspace` = 244 passed, 0 failed.

Status: `[ ]` not started · `[~]` in progress · `[x]` complete · `[-]` intentionally skipped

See [`docs/testing/user-journeys.md`](docs/testing/user-journeys.md) and
[`docs/architecture/implementation-map.md`](docs/architecture/implementation-map.md).

---

## P0 — clean product-level logging

The brief requires `RUST_LOG=flow=debug` to give useful product logs with **no**
`tokio_tungstenite` / frame / byte-dump noise, and to **not** require `RUST_LOG=trace`.
Today the daemon ignores `RUST_LOG` entirely (`LevelFilter` reload layer) and `FLOW_TRACE`
turns on the global TRACE firehose.

- [x] **L1** — `daemon/Cargo.toml`: `tracing-subscriber` `features = ["env-filter"]`.
- [x] **L2** — `logging.rs`: `reload::Layer<LevelFilter>` → `reload::Layer<EnvFilter>`.
      `RUST_LOG` set → wins verbatim (`base` as default directive). Unset → scoped
      `flow={lvl},flow_daemon={lvl}`. `FLOW_TRACE` → `flow=trace,flow_daemon=trace`.
- [x] **L3** — `LoggingHandle` gained `trace_floor: bool` + `env_controlled: bool`;
      `set_debug` reloads a scoped `EnvFilter`, or one-time no-op when `env_controlled`.
      `current_level()` → `max_level()`. 6 tests + 2 helpers rewritten, +1 new.
- [x] **L4** — `logging::product`: `input` (DEBUG, per-event), `switch`/`peer_connected`/
      `peer_disconnected` (INFO), `error` (ERROR); all `target: "flow"`, ASCII `->`.
- [x] **L5** — wired: `[INPUT]` both directions in `run_paired_connection` (+`describe_event`);
      `[SWITCH]` in both `service` switch paths; `[PEER]` in `main::run_peer_pipeline`;
      `[ERROR]` at inject/suppress/capture failure sites in `pipeline` + `main`.
- [x] **L6** — `cargo test --workspace` 244→245, `clippy -D warnings`, `fmt` green.
      Smoke runs (unset / `flow=debug` / `FLOW_TRACE=1`): readable startup, zero
      `tungstenite`/`tokio_tungstenite`/`Framed`/byte-dump lines in all three.
- [x] **L7** — `daemon/README.md` logging section + `todos-fix-physical-input-switching.md`
      §1 updated.

## P0 — macOS local suppression (Journey R — the one code gap)

Without this, while the Mac is master it types into **both** machines.
Cannot be executed here (no Mac) — unit-test + cross-compile-check, then validate on the
maintainer's Mac with a lifeline.

- [x] **M1** — `suppress: Arc<AtomicBool>` on `MacosInputCapture`, cloned into the capture
      loop; `set_suppress_local` → `store(SeqCst); Ok(())`; `SuppressionUnsupported` removed.
- [x] **M2** — active tap (`CGEventTapOptions::Default`); `TapDisabledBy{Timeout,UserInput}`
      re-armed via `CGEventTapEnable` from inside the callback. (They're in
      `events_of_interest()` for documentation; `event_mask()` folds them out since their
      discriminants aren't valid shift amounts — the OS delivers them anyway.)
- [x] **M3** — raw `mod ffi` (`CGEventTapCreate` + `extern "C"` trampoline) returning
      `ptr::null_mut()` for a withheld event; `core-foundation` keeps the run-loop source.
      All `unsafe` in the trampoline + `run_capture_loop` + `mod ffi`, each `SAFETY:`-doc'd.
      Callback fails **open** (`catch_unwind` → pass the event through). Added
      `foreign-types = "0.5"` (for `ForeignType::from_ptr` to borrow `&CGEvent`).
- [x] **M4** — `SuppressionGate` ported (`HashSet<i64>` keycodes + `HashSet<u8>` buttons);
      modifiers derived from the `FlagsChanged` flag bitmask like `EventTranslator`. 11 unit
      tests (`cfg(target_os="macos")`).
- [x] **M5** — injector stamps `EVENT_SOURCE_USER_DATA = FLOW_INJECTED_MARKER`
      (`macos/mod.rs`); the tap passes marked events through without forwarding or gating.
- [x] **M6** — `cargo check -p flow-core -p flow-platform --target x86_64-apple-darwin` +
      `--target aarch64-apple-darwin` + `clippy --target x86_64-apple-darwin -D warnings`:
      all green. Native `cargo test --workspace` unchanged (245, macOS tests are cfg'd out).
- [x] **M7** — `daemon/README.md` "Local input suppression" (macOS row → real, unverified),
      `core/src/input/mod.rs` trait doc, `todos-fix-physical-input-switching.md` §7 updated.

  ⚠ **Two things only a real Mac can confirm:** (1) that returning `NULL` actually drops the
  event on the running macOS version; (2) that `EVENT_SOURCE_USER_DATA` survives
  `CGEventPost` (if not → Mac-as-master echoes its own input; fallback = a `getpid()` check).
  Both are called out in `physical-test-script.md` Round 2 with a pre-flight probe.

## P0 — physical validation (maintainer, two machines)

- [ ] **V1** — produce `docs/testing/physical-test-script.md`: exact daemon + UI launch
      commands for Windows and Mac (env vars, ports), and the ordered acceptance checklist
      (the brief's 18 points), split into **Round 1: Windows-as-master** (no new code —
      exercises the merged Windows suppression) and **Round 2: Mac-as-master** (after M*,
      with the SSH lifeline procedure).
- [ ] **V2** — maintainer runs Round 1; report back `FLOW_TRACE` logs + checklist results.
- [ ] **V3** — maintainer runs Round 2; report back.
- [ ] **V4** — triage #4 (connection ownership) and #5 (WebSocket 1006) against the real
      logs from V2/V3; smallest fixes, ref-checked.

## Found by running the app (iteration 3)

- [x] **G1** — Windows capture re-captured the daemon's own `SendInput` output (no
      self-injection guard; Linux skips its uinput node, macOS got one in iteration 2).
      As a slave, re-captured events re-entered the pipeline and echoed back to the active
      peer; relative `MouseMove` deltas compounded → 14 synthetic events became 212k+.
      Fix: `dwExtraInfo = FLOW_INJECTED_MARKER` on every injected `INPUT`; `keyboard_proc`
      / `mouse_proc` skip matching events. Verified: `drive_two_daemons` → ALL CHECKS
      PASSED, 14/14 each direction (was 0 / 212771).

## P1 — follow-ups (after physical validation)

- [ ] **F1** — Journey 10: repeated-switching drift check on real hardware (no stuck
      keys/buttons, no dup input, no crash across many cycles). Fix what surfaces.
- [ ] **F2** — Journey 5: complete a real Windows ↔ Mac pairing (maintainer presses Pair on
      the Mac); confirm mutual trust persists across restart on both.
- [ ] **F3** — Journey 11/12: exercise a real peer drop; confirm link state →
      `Reconnecting` → recovery in the UI, local input usable throughout.
- [ ] **F4** — multi-key switch binding leaks modifier down/up to the peer
      (`spawn_pipeline_switch_filter`). Default Scroll Lock is clean; fix or document as a
      known limitation for non-default bindings.

## Out of scope for V1 `[-]`

- [-] Multi-peer routing / broadcast / >1 active destination / routing graphs
- [-] Bluetooth transport wiring · Virtual HID · protocol redesign
- [-] Clipboard / file transfer / mouse-position switching / >2 computers
- [-] `InputRoute` enum (prior investigation: would duplicate the already-atomic
      single-`Active` invariant — no change needed)
- [-] The >40 s killed-daemon detection after multiple IPC connections — pre-existing,
      separate from V1; note only.

---

## Iteration log

| # | Problem | Root cause | Fix | Test | Result |
|---|---------|-----------|-----|------|--------|
| 0 | baseline | — | — | `cargo test --workspace` | 244 pass, 0 fail; build/clippy/fmt green |
| 1 | `RUST_LOG=flow=debug` ignored; `FLOW_TRACE` = global TRACE (tungstenite firehose) | `logging.rs` used a single global `LevelFilter` reload layer, no per-target scoping; `env-filter` feature not enabled | `EnvFilter` reload layer; scoped `flow`/`flow_daemon` default; `RUST_LOG` honored when set; `logging::product` `[INPUT]/[SWITCH]/[PEER]/[ERROR]` lines wired into pipeline/service/main | `cargo test --workspace` (245), `clippy -D`, `fmt`, 3 smoke runs | green; zero dep/frame noise at any level; product lines readable |
| 2 | Mac-as-master types into both machines (Journey R) | `macos/capture.rs` used a `ListenOnly` `CGEventTap` (can't swallow); `set_suppress_local` returned `Err(SuppressionUnsupported)` | Active tap via raw `CGEventTapCreate` + `extern "C"` trampoline returning `NULL` to drop; `Arc<AtomicBool>` flag; `SuppressionGate` port; `TapDisabledBy*` re-arm; self-inject marker guard; fails open on panic | native `cargo test --workspace` (245), apple `cargo check`/`clippy` x2 targets, 11 gate unit tests (cfg'd, hand-verified) | code-complete + cross-checked; **2 items pending Mac hardware** (NULL-drop contract, marker survival) |
| 3 | 14 synthetic events → 212k+ forwarded, exp. growing mouse deltas (found running `drive_two_daemons`) | Windows LL hooks re-captured the daemon's own `SendInput` output; as a slave those re-entered the pipeline and echoed to the active peer, compounding via the relative→absolute→relative move translation | `dwExtraInfo = FLOW_INJECTED_MARKER` on every injected `INPUT`; `keyboard_proc`/`mouse_proc` skip matching events (mirrors macOS M5, Linux's uinput-node skip) | `cargo test --workspace` (246), clippy -D, fmt; `drive_two_daemons` re-run | ALL CHECKS PASSED — 14/14 each direction, switch halts flow, `[INPUT]`/`[SWITCH]`/`[PEER]` logs correct |
