# flow-daemon

The Rust data-plane daemon for Flow — input capture, injection, device switching, networking, and pairing, running independently of the Flutter UI (`docs/product/vision.md` §8). This directory holds the daemon binary itself; the workspace also includes `flow-core` (`../core`) and `flow-platform` (`../platform`).

**Status: scaffolding.** `cargo build --workspace` succeeds today, but `flow-daemon` only constructs an empty `AppState` and prints a placeholder line — no IPC, no input capture, no networking yet. See [`todos.json`](./todos.json) and [`PLAN.md`](./PLAN.md) for the full build-out plan; this file will be rewritten track by track as it lands, the same way `flutter/README.md` was rewritten once the UI was real.

## Why this exists separately from the root README

The root [`README.md`](../README.md) covers the whole product and points here for daemon specifics, the same way it points to [`flutter/README.md`](../flutter/README.md) for UI specifics. Once the daemon is real, this file is where "how do I run it, what does it actually do today, what's still stubbed" lives.

## Workspace layout

```
core/       flow-core     — protocol, device, pairing, transport, state, input traits (no OS/transport/UI deps)
daemon/     flow-daemon   — this binary; wires flow-core + flow-platform together, owns the IPC server
platform/   flow-platform — per-OS input adapters (macos/, windows/, linux/) behind flow-core's traits
```

## Building and running

```sh
cargo build --workspace
cargo run -p flow-daemon
```

Today this prints a scaffolding placeholder and exits. Once track C (`local-ipc-transport`) lands, it will bind a WebSocket listener on `127.0.0.1:47823` (`docs/contracts/daemon-ipc.md`'s local IPC contract) and stay running until interrupted.

## Testing and linting

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

## The contract this daemon is building toward

[`docs/contracts/`](../docs/contracts) defines the exact interface — commands, state streams, error codes, state machines — that `flutter/lib/data/mock_daemon_repository.dart` already implements on the Flutter side. `flow-daemon` exists to become a second, real implementation of that same contract reachable over local IPC, so `flutter/lib/state/repository_providers.dart` can eventually point at it instead of the mock with **no UI code changing** (`docs/contracts/README.md` ground rule 2). `daemon/todos.json` tracks A-D build exactly this, in order.

## Platform adapters: what's real vs. stubbed

| Platform | Capture | Injection | Verified how |
|---|---|---|---|
| Linux | pending (`todos.json` E1) | pending (E2) | buildable and testable in a standard Linux dev container; needs `/dev/input` + `uinput` access, documented per-task once implemented |
| macOS | pending (E4) | pending (E5) | written against `core-graphics`, verified with `cargo check --target <apple-target>` only — no Mac hardware in this development environment |
| Windows | pending (E6) | pending (E7) | written against the `windows` crate, verified with `cargo check --target <windows-target>` only — no Windows hardware in this development environment |

Cross-compilation setup instructions land in `todos.json` task J3 once the macOS/Windows adapters exist to check.

## Persistence

Settings, paired devices (which double as the trust store), this daemon's own identity keypair, and a connection history log all live in a single local SQLite database (`rusqlite`, bundled — no system SQLite dependency) under the platform data directory, applied via versioned migrations on startup. Nothing here is derived fresh on every run or held only in memory: a fresh database bootstraps to the same seed data the mock uses (3 devices, defaults), and every subsequent run loads what was actually persisted. `daemon/todos.json` track **P** (`persistence-storage`) builds this, positioned right after the core contract types and ahead of the command service itself, since `DaemonService`'s startup state depends on it.

## Process supervision (not yet implemented)

The daemon is meant to run independent of any UI, ideally started at login/boot and restarted automatically if it crashes (`docs/product/vision.md` principle 7). This phase scaffolds but does not install service unit files (`todos.json` J4: a `launchd` plist, a `systemd` unit, notes for a Windows service wrapper) — actually registering the daemon to auto-start is future work beyond this plan.

## Security posture during this phase

The local IPC channel (Flutter <-> this daemon) is bound to `127.0.0.1` only and assumes it's reachable solely by the local user, per `docs/contracts/README.md`'s scope note — it does not carry its own authentication in v0.1. The daemon-to-daemon network channel (once track G/H land) is the one that actually carries sensitive keyboard/mouse data across the LAN, and is where real device identity, trust, and encryption (`docs/product/vision.md` §17) apply.

## Manual verification notes

Several tasks in `todos.json` (E1-E3, E4-E7, G4, I4) can only be fully verified with real input devices, a second machine, or platform hardware this development environment doesn't have. Each such task's acceptance criteria says explicitly what was verified automatically (unit tests on pure translation logic, `cargo check` for cross-compiled platforms, integration tests against synthetic events) versus what still needs a human with the actual hardware to confirm. This section will grow with concrete "how to manually verify" steps as those tasks land.
