# Flow

**One keyboard. One mouse. Multiple computers.**

Flow is a cross-platform utility that lets you seamlessly control multiple computers with a single keyboard and mouse, switching the active machine with a dedicated keyboard shortcut — no specialized multi-device peripherals required.

## The problem

Power users and developers often work across multiple computers at once — a MacBook for personal work, a Windows/Linux machine for work or experimentation — which usually means two keyboards, two mice, extra cables and receivers, and a cluttered desk. Commercial multi-device peripherals solve this, but only if you buy new hardware.

Flow recreates that experience entirely in software, reusing the keyboard and mouse you already own.

## How it works

Every computer runs Flow. One is the **controller**: it shares its physical keyboard and mouse. The others are **controlled**: they are driven by a controller. You move between screens with a shortcut (**Scroll Lock** by default), a "come back" shortcut, or by pushing the pointer through a screen edge. The role is a setting per computer, not a baton that passes around; see [ADR 0001](docs/adr/0001-controller-controlled-input-model.md).

```
   Controller (has the keyboard + mouse)            Controlled
   ┌──────────────────────────┐   encrypted LAN   ┌─────────────┐
   │ captures input, decides  │──────────────────►│ injects it  │
   │ which screen it goes to  │◄──────────────────│             │
   └──────────────────────────┘   heartbeat/Leave └─────────────┘
```

## Product principles

- **Invisible by default** — you should forget the software is running.
- **Instant interaction** — switching computers should feel immediate.
- **Native system integration** — behaves like an OS utility, not a traditional app.
- **Zero physical clutter** — the software UX should reduce clutter, not add to it.
- **Cross-platform first** — macOS, Windows, and Linux are all first-class.
- **Progressive complexity** — simple by default, configurable for power users.
- **UI and engine are separate** — the engine keeps running when the window is closed, and runs headless without any UI.

## Architecture

One engine, two ways to host it.

```
┌──────────────────────────────────────────────┐   ┌──────────────────────┐
│ Flow desktop app (Tauri 2, app/)             │   │ flow-daemon (headless)│
│  React + TypeScript UI (app/ui) on the       │   │  servers, CI, tests   │
│  @flow/design-system package                 │   │  loopback WebSocket   │
│  tray menu + popover, notifications,         │   │  + token auth         │
│  launch at login, permissions                │   └──────────┬───────────┘
│  engine runs IN-PROCESS: no local socket     │              │
└───────────────────────┬──────────────────────┘              │
                        │ same commands, same events          │
┌───────────────────────▼─────────────────────────────────────▼──────┐
│ Engine library (daemon/)                                           │
│  Engine::start → DaemonService: input routing, switching, pairing, │
│  trust, discovery, per-peer links, settings, permissions,          │
│  diagnostics. One dispatcher, one event stream, one config builder │
└───────────────────────┬────────────────────────────────────────────┘
                        │ PlatformBackend (native or virtual)
              flow-platform: macOS · Windows · Linux input adapters
```

- **The engine owns everything the computer does**: global input capture and injection, hotkeys and edge switching, keymaps and remaps, networking (Noise + Ed25519, TCP, discovery), pairing and trust, OS permission probing. Platform differences live behind the `PlatformBackend` trait, so the engine also runs on a virtual backend with no input devices.
- **The desktop app is a thin shell**: it embeds the engine, bridges `flow_command` / `flow://event` to the webview, and adds the tray, notifications and OS integrations. Closing the window hides it; Quit stops the engine. A second launch just shows the running app.
- **The UI talks to an engine through `FlowClient`** (`app/ui/src/client`), which speaks the contract in [`docs/contracts/`](docs/contracts) over a transport: Tauri (production), WebSocket (`?daemon=ws://…` against `flow-daemon`, or the app with `FLOW_DEV_WS=1`) or an in-memory mock (`?scenario=…`) for development and tests.
- **Keymaps and remap rules are configuration**, not code: defaults in `config/keymaps/`, user overrides in the config folder (**Open keymap folder** in the app).
- **Design system**: `design-system/` is a reusable package (tokens, components, gallery); see its README and GUIDELINES.

See [`docs/redesign/PLAN.md`](docs/redesign/PLAN.md) for the 2.0 redesign and [`docs/product/vision.md`](docs/product/vision.md) for the original vision (historical where it disagrees).

## Project layout

```
app/src-tauri/  flow-app — the Tauri shell: engine host, command/event bridge,
                tray, notifications, autostart, bundling (dmg/msi/nsis/deb/AppImage)
app/ui/         React + TypeScript UI, FlowClient + transports, Vitest + Playwright
design-system/  @flow/design-system — tokens, components, gallery
core/           flow-core — protocol, device, control model, pairing, settings,
                IPC envelope, input traits (no OS or transport code)
daemon/         flow-daemon — the engine library and the headless binary
platform/       flow-platform — per-OS input adapters and permission probes
config/         keymaps/ — default per-OS key tables and remap rules (TOML)
docs/           contracts/ (UI↔engine), adr/, redesign/, architecture/, archive/
```

## Quick start

From a clean checkout (Rust via rustup, Node 22, and on Linux the packages listed in [`CONTRIBUTING.md`](CONTRIBUTING.md#setup)):

```sh
npm --prefix app/ui ci                  # UI dependencies
npm --prefix app/ui run build           # the UI the app embeds
cargo run -p flow-app                   # the desktop app (engine in-process)
cargo run -p flow-daemon                # or: the headless engine (loopback WebSocket + token)
just dev                                # or: the app with UI hot reload (tauri dev, run from app/)
npm --prefix app/ui run dev             # the UI alone in a browser, on mock data (http://localhost:5173)
```

**Network and logs.** Flow uses UDP `47824` (discovery) and TCP `47825` (connections) on the local network; on Linux the in-app setup opens them in ufw/firewalld. Logs are written to `<data dir>/logs/flow.log` (Linux: `~/.local/share/flow-daemon/logs/flow.log`; macOS: `~/Library/Application Support/dev.Flow.flow-daemon/logs/flow.log`).

Tests: `cargo test --workspace`, `npm --prefix app/ui test`, `npm --prefix app/ui run e2e`, and `just harness` for the
multi-device scenario matrix (no hardware needed). See `daemon/README.md` and `app/ui/README.md`.

## Development

Toolchain pinned via `rust-toolchain.toml`; common tasks run through `just` (or `./scripts/dev.sh <verb>`):
`just setup`, `just lint`, `just test`, `just ci`. See [`CONTRIBUTING.md`](CONTRIBUTING.md) for setup per OS,
commit conventions and the PR checklist.

## POC roadmap

The proof of concept validates the core interaction before investing in full platform complexity:

1. **Connectivity** — establish a reliable connection between two daemons. ✅ Implemented in the real Rust daemon (`docs/archive/tasks/daemon-todos.json` track G): TCP discovery, medium negotiation, and a real pairing handshake all work end to end, and are now wired into `main.rs` itself — two independently-started `flow-daemon` processes on the same network discover, pair, and reconnect each other with no test harness gluing the pieces together. Verified by automated tests against real sockets and by manually running the built binary in this container; not yet verified across two genuinely separate physical machines (this environment only has the one). Bluetooth discovery/channels remain standalone, tested building blocks behind the `bluetooth` feature, not wired into `main.rs` — see `daemon/README.md`'s top status line for the exact split.
2. **Keyboard** — capture and transmit keyboard events. ◑ Real per-OS capture/injection (`flow-platform`, tracks E1/E4/E6) and a real streaming pipeline (track G8) exist and are wired into the daemon. The remaining gap is *local suppression*: capture is passive, so the sending machine must also stop its own OS processing the keystrokes it forwards. That's implemented on Linux (exclusive `EVIOCGRAB`) but **not on macOS or Windows**, where input currently reaches both machines at once. Only Linux capture/injection has run against real hardware in this project's own development environment (no macOS/Windows machine, no `/dev/input` in this container) — see "Local input suppression" and "Platform adapters" in `daemon/README.md`.
3. **Mouse** — add movement, clicks, and scroll. ✅ Same `InputEvent`/pipeline covers mouse alongside keyboard from the start (`core::protocol::MouseEvent`) — not a separate later addition in the Rust implementation.
4. **Switching** — implement Scroll Lock-based device switching. ✅ `docs/archive/tasks/daemon-todos.json` track F: a configurable switch-key binding (Scroll Lock is one of several presets), detected locally without needing any UI running.
5. **Desktop app** — status, pairing, switch key, settings, tray. ✅ The Tauri app (`app/`) embeds the engine; the earlier Flutter client was retired (history in `docs/archive/`).
6. **Reliability** — stress-test network interruptions, restarts, and rapid switching. ◑ Structured logging and per-task panic isolation exist and are tested (`docs/archive/tasks/daemon-todos.json` track I); real peer connections recover on their own via discovery's periodic re-announce redialing a paired device once it's reachable again (not `channel::reconnect::maintain_connection`'s exponential-backoff loop, which remains a standalone, tested building block `main.rs` doesn't call directly — see `daemon/README.md`), with a deterministic tie-break so two daemons starting simultaneously don't drop each other's connections. Genuine multi-hour/rapid-switching stress testing against real hardware hasn't happened — this environment can't run two real daemons against each other continuously.
7. **Native daemon** — harden the POC into the production Rust daemon across macOS, Linux, and Windows. ✅ Every task in `docs/archive/tasks/daemon-todos.json` (tracks A-J) is done; see `daemon/README.md` for the full, honest account of what's verified how on each OS.

## Success criteria

**POC:** two computers connect reliably, keyboard/mouse events are transmitted with low perceptible latency, Scroll Lock switching feels instantaneous, and the system runs continuously without excessive resource usage. *Status: implemented and unit-tested end to end on the TCP medium, but never demonstrated on two physical machines, and local input suppression — without which input reaches both computers instead of just the active one — is Linux-only so far.*

**Product:** installation takes minutes, pairing is nearly effortless, switching is instant, the daemon is effectively invisible, the UI feels native on every OS, and users never have to think about which computer their peripherals are connected to.

## North star

One desk. Multiple computers. One keyboard. One mouse. No hardware switching, no extra peripherals, no messy desk, no complicated setup — just press a key and continue working.
