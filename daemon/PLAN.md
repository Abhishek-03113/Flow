# Rust Daemon Development Plan

Branch: `feat/rust-daemon` (off `feat/flutter-ui`) · Companion data file: [`daemon/todos.json`](./todos.json) · Contract: [`docs/contracts/`](../docs/contracts) v0.1.0

This is the Rust-side counterpart to the Flutter phase's `todos.json`. Same discipline: one track per concern, tasks with explicit dependencies, a spec excerpt embedded in every task so it's resumable cold, one commit per completed task, `status` flipped `pending -> done` as work lands.

## Where this starts from

`core/`, `daemon/`, `platform/` already exist as a Cargo workspace — scaffolded (see `Scaffold Rust daemon workspace and Flutter UI`) alongside the Flutter work, not built yet:

- `flow-core` — `Device`/`DeviceId`/`DeviceState`, `InputEvent`/`KeyboardEvent`/`MouseEvent`, `PairingRequest`/`PairingDecision`, `InputCapture`/`InputInjector`/`Transport` traits, a bare `AppState`. No `HostOs`, `DaemonLinkState`, `PairingSession`, `FlowSettings`, `PermissionStatus`, or error types yet — the contract's full vocabulary isn't there.
- `flow-daemon` — a `main.rs` that constructs `AppState` and prints a placeholder line. No IPC, no command handling.
- `flow-platform` — `LinuxInputCapture`/`MacosInputCapture`/`WindowsInputCapture` (+ injectors) all `unimplemented!()`.

The Flutter UI, meanwhile, is **complete** against `MockDaemonRepository` (49 tests passing, all 31 Flutter-phase tasks done). Nothing here changes Flutter code except track D, which adds a second `DaemonRepository` implementation without touching a single screen.

## Why this order

```
A  core-contract-parity ─> P  persistence-storage (SQLite) ─┬─> B  daemon-command-service ─> C  local-ipc-transport ─> D  flutter-ipc-adapter
                                                              │
                                                              └─> E  platform-input-adapters ─┬─> F  switch-hotkey ─┐
                                                                                                │                    ├─> G  transport-networking ─> H  security-pairing-trust ─┐
                                                                                                └────────────────────┘                                                         │
                                                                                                                                             I  reliability-persistence <───────┤
                                                                                                                                                                                  │
                                                                                                                                             J  packaging-qa <──────────────────┘
                                                                                                                                             (depends on everything above)
```

**A -> P -> B -> C -> D is the priority path.** It's the Rust equivalent of what the Flutter phase did with the mock: get a fully contract-correct, durably-stored, no-OS-dependencies service running and reachable over the wire before touching a single platform API. Track B's `DaemonService` reuses `MockDaemonRepository`'s exact seed data and pairing timings as a *first-run bootstrap* on purpose — the point isn't a different mock, it's proving the *same* observable behavior is implementable in Rust, backed by a real database, and reachable from Flutter over real IPC. D is the payoff: swap `daemonRepositoryProvider` from `MockDaemonRepository` to `IpcDaemonRepository` and nothing in `lib/features/` should need to change.

**P sits between A and B, not after everything else.** The original draft of this plan deferred persistence to a late "reliability" task using flat JSON files — settings and paired devices would live in memory until that task landed, and connection history wasn't tracked at all. That's backwards for a daemon meant to run continuously: settings, paired devices (which double as the trust store), this device's own identity keypair, and a connection history log all need to survive a restart from the start, not be bolted on once everything else works. Track P is a single SQLite database (`rusqlite`, bundled/statically-linked so cross-compiling to macOS/Windows from this container doesn't need a system SQLite) with four tables — `settings`, `devices`, `connection_history`, `identity` — and `B1` loads from it (falling back to the mock's seed data only on an empty, first-run database) instead of always reseeding in memory.

**E (platform input) runs in parallel with B/C**, not after — it only depends on A (the shared `InputEvent`/`HostOs` vocabulary), not on the service or IPC layer. Linux (E1-E3) is buildable and unit-testable in this container; macOS/Windows (E4-E7) are cross-compile-checked only, since there's no Mac or PC available here — flagged explicitly in each task's acceptance criteria so "done" never silently means "assumed to work."

**F needs both E (real capture) and B (a service to call)** — it's where the daemon stops being a passive command-responder and starts reacting to raw input on its own, which is the actual point of the product (vision.md: "switching is the defining interaction").

**G (networking) needs E and F** — you can't stream real input between two daemons until each one can capture/inject locally and knows when it's the active one. **H (security) needs G** — there's nothing to encrypt until there's a real link. **I (reliability)** cuts across C/G and is scheduled after them rather than interleaved, since retry/persistence logic is much easier to write correctly against transports that already work than to build alongside them.

**J is last and depends on all of it**, mirroring the Flutter phase's QA track: lint discipline, a full test sweep, cross-compilation documentation, packaging scaffolding, and a final doc reconciliation pass so nothing in `docs/contracts/`, the root `README.md`, or `daemon/README.md` claims something the code doesn't actually do.

## Track summary

| Track | Tasks | Depends on | What "done" means |
|---|---|---|---|
| **A** — core-contract-parity | 7 | — | `flow-core` types are a 1:1, serde-wire-verified mirror of `docs/contracts/data-model.md` |
| **P** — persistence-storage | 6 | A | Settings, paired devices/trust, identity, and connection history all live in a real SQLite DB |
| **B** — daemon-command-service | 7 | A, P | `DaemonService` loads from SQLite (seeding only a fresh DB) and passes a suite mirroring the Dart mock's 13 cases |
| **C** — local-ipc-transport | 6 | B | `flow-daemon` is a real process; a raw WebSocket client can drive the full contract against it |
| **D** — flutter-ipc-adapter | 5 | C | `IpcDaemonRepository` exists in Flutter, swappable via a flag, mock stays the default |
| **E** — platform-input-adapters | 7 | A | Linux capture/injection real and tested; macOS/Windows written + cross-compile-checked |
| **F** — switch-hotkey | 3 | E, B | A configured switch key actually switches the active device, with no UI attached |
| **G** — transport-networking | 4 | E, F | Two daemon instances discover, pair, and stream real input end to end (Linux, local test) |
| **H** — security-pairing-trust | 4 | G, P | Daemon-to-daemon traffic is identity-keyed (persisted), Noise-encrypted, replay-protected |
| **I** — reliability-persistence | 3 | C, G, P | Link state recovers from a dropped connection; daemon panics are isolated per-task |
| **J** — packaging-qa | 5 | everything | Clean clippy/fmt, full test sweep, cross-compile docs, service-unit scaffolding, docs reconciled |

## Key design decisions baked into the plan

- **Persistence is SQLite (`rusqlite`, bundled), not flat JSON files, and not in-memory-until-later.** Settings, paired devices (which double as the trust store — a device's stored public key *is* its trust record), this device's own identity keypair, and a connection history log all live in one database with a versioned schema, wired in from track P onward rather than bolted on late. Connection history is captured by a background task that *observes* `DaemonService`'s existing watch-channel event bus and diffs state transitions into log rows — no command handler needs to remember to log anything explicitly.
- **IPC transport: WebSocket over loopback TCP, port `47823`** (not a Unix socket / named pipe). One implementation instead of three `#[cfg(target_os)]` branches for v0.1; reachability is restricted to `127.0.0.1` as the security boundary for now, with socket-based hardening explicitly deferred to track H's neighborhood, not forgotten (see `docs/contracts/daemon-ipc.md` after C5).
- **`tokio::sync::watch` for the 5 state streams** — it natively replays the latest value to a newly-subscribing receiver, which is exactly the semantics `MockDaemonRepository`'s `_StateChannel` needed a `Stream.multi` rewrite to get right on the Dart side (the `async*` version silently dropped events in a race). Rust gets this for free from the stdlib-adjacent primitive; worth calling out since it's a case where the two languages' idiomatic answer to the same contract requirement genuinely differs.
- **Mock-parity timings are deliberately copied into `DaemonService`** (1200ms/1500ms/1600ms) even though `daemon-ipc.md` says they're not part of the contract for a *production* daemon. The reason is development ergonomics during this phase: the observable behavior of "mock" vs. "real daemon over IPC" should be indistinguishable while D is being built and tested, so a swap never looks like a regression. Track I or a later phase can detach real timings from these constants once actual network/pairing latency exists.
- **`FlowError` centralizes all 9 contract error codes** in `flow-core` (`error::code()`), so no command handler in `flow-daemon` ever hand-writes an error string — the same discipline `docs/contracts/README.md` ground rule 1 asks of the Dart side (`DaemonCommandException`).
- **Linux is the only platform this session can actually build, run, and test.** macOS and Windows adapters (E4-E7) are written for correctness and checked with `cargo check --target ...`, not run — every task says so explicitly rather than letting "implemented" imply "verified."

## What's explicitly out of scope for this phase

Matches `docs/contracts/daemon-ipc.md`'s own "deliberately out of scope for 0.1.0" list, extended to the daemon build:

- Multi-hop / >2 device topologies and mouse-position-based auto-switching.
- A shipped installer or auto-start registration (J4 only *scaffolds* service unit files — it does not install or activate them).
- Diagnostics data (Advanced settings' "average switch time, dropped input" line) — still static copy in the UI; wiring it up needs the daemon to actually measure those numbers, which isn't in this plan either.
- Any UI change beyond track D's provider wiring — every other Flutter screen is finished and untouched.

## How to pick this up mid-stream

Read `daemon/todos.json`, find the first task with `"status": "pending"` whose `dependsOn` are all `"done"`, and its `specContext` field has the spec quote needed to implement it without re-reading this whole plan. Update `status` to `"done"` and fill in a `buildNote` (mirroring the Flutter phase's field) only when the implementation deviated from what the task described — commit with the task's `commit` message (adjusted if the implementation diverged) after each task, one commit per task, and push.
