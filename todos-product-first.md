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

- [ ] **M1** — `platform/src/macos/capture.rs`: add `suppress: Arc<AtomicBool>` on
      `MacosInputCapture` (survives stop/start), cloned into the capture loop. Replace
      `set_suppress_local`'s hard `Err` with `store(SeqCst); Ok(())`. Remove
      `MacosCaptureError::SuppressionUnsupported`.
- [ ] **M2** — active tap: `CGEventTapOptions::Default`; add
      `kCGEventTapDisabledByTimeout` / `…ByUserInput` to `events_of_interest()` and re-arm
      (`CGEventTapEnable`) the tap when they arrive.
- [ ] **M3** — event drop: `core-graphics` 0.24's safe wrapper can't drop an event. Use a
      raw-FFI `CGEventTapCreate` trampoline returning `NULL` for a withheld event, keeping
      `core-graphics`/`core-foundation` for field access + the run-loop source. Isolated
      `unsafe`, documented.
- [ ] **M4** — `SuppressionGate` port from `windows/capture.rs` (`HashSet<CGKeyCode>` +
      `HashSet<button>`): withhold a KeyUp/ButtonUp **iff** its down was withheld, so a
      mid-hold toggle or the switch key's own release never strands a half-press locally.
      Drive modifiers off the `FlagsChanged` press/release diff.
- [ ] **M5** — self-injected-event guard: an active HID tap re-sees this machine's own
      `CGEventPost` output. Tag injected events (source user-data) and skip them in the
      callback so a machine that is briefly both sender and receiver doesn't loop.
- [ ] **M6** — unit tests for `SuppressionGate` (no hardware); `cargo check`/`clippy`
      `--target x86_64-apple-darwin` + `aarch64-apple-darwin` for `flow-platform`.
- [ ] **M7** — update `daemon/README.md` "Local input suppression" (macOS row → real),
      `core/src/input/mod.rs` trait doc, `todos-fix-physical-input-switching.md` §7.

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
