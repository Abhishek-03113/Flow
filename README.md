# Flow

**One keyboard. One mouse. Multiple computers.**

Flow is a cross-platform utility that lets you seamlessly control multiple computers with a single keyboard and mouse, switching the active machine with a dedicated keyboard shortcut — no specialized multi-device peripherals required.

## The problem

Power users and developers often work across multiple computers at once — a MacBook for personal work, a Windows/Linux machine for work or experimentation — which usually means two keyboards, two mice, extra cables and receivers, and a cluttered desk. Commercial multi-device peripherals solve this, but only if you buy new hardware.

Flow recreates that experience entirely in software, reusing the keyboard and mouse you already own.

## How it works

Each computer runs a lightweight background daemon that captures keyboard and mouse events, sends them to (or receives them from) another computer, and injects remote input locally. Pressing a configurable shortcut — **Scroll Lock** by default — switches which computer is currently receiving your input.

```
        ┌─────────────┐               ┌─────────────┐
        │   Device A  │◄─────────────►│   Device B  │
        │    Active   │   Transport   │   Inactive  │
        └─────────────┘               └─────────────┘
```

## Product principles

- **Invisible by default** — you should forget the software is running.
- **Instant interaction** — switching computers should feel immediate.
- **Native system integration** — behaves like an OS utility, not a traditional app.
- **Zero physical clutter** — the software UX should reduce clutter, not add to it.
- **Cross-platform first** — macOS, Windows, and Linux are all first-class.
- **Progressive complexity** — simple by default, configurable for power users.
- **UI and system functionality are separate** — the daemon keeps working even if the UI is closed.

## Architecture

Flow is split into two layers: a Flutter UI (control plane) and a Rust daemon (data plane).

```
┌───────────────────────────────────────────┐
│                Flutter UI                 │
│  Menu Bar / System Tray, Device Management │
│  Pairing, Settings, Connection Status      │
└─────────────────────┬─────────────────────┘
                      │ Local IPC / API
┌─────────────────────▼─────────────────────┐
│             Native Daemon (Rust)          │
│  Input Capture, Input Injection            │
│  Device Switching, Networking, Pairing     │
│  Security, Configuration, Device State     │
└─────────────────────┬─────────────────────┘
                      │ Transport Layer
          ┌───────────┼───────────┐
          ▼           ▼           ▼
      WebSocket    Bluetooth     Future
```

- **Flutter** owns everything the user sees: menu bar/tray UI, pairing, settings, onboarding, and status.
- **Rust** owns everything the computer actually does: global input capture/injection, hotkeys, networking, pairing, encryption, and OS-specific integrations, isolated behind per-platform adapters (macOS/Windows/Linux).
- The daemon runs standalone and keeps working even if the UI is closed — the Flutter UI only configures, pairs, and observes it over local IPC.
- The transport layer is abstracted from day one, starting with WebSocket over the local network and leaving room for Bluetooth, QUIC, and other transports later.

See [`docs/product/vision.md`](docs/product/vision.md) for the full product vision, architecture rationale, event protocol, security model, and POC roadmap.

## Project layout

```
core/       flow-core — protocol, device, pairing, transport, state, and
            input-capture/injection traits (no OS or transport code)
daemon/     flow-daemon — the daemon binary; wires core + platform together
platform/   flow-platform — per-OS input adapters (macos/, windows/, linux/),
            each implementing the traits from core
flutter/    Flutter control-plane app (devices, onboarding, settings, tray,
            services); see flutter/README.md
```

Build the daemon workspace with `cargo build --workspace`; see `flutter/README.md` for the UI.

## POC roadmap

The proof of concept validates the core interaction before investing in full platform complexity:

1. **Connectivity** — establish a reliable connection between two daemons. ✅ Implemented in the real Rust daemon (`daemon/todos.json` track G): TCP discovery, medium negotiation, and a real pairing handshake all work end to end, and are now wired into `main.rs` itself — two independently-started `flow-daemon` processes on the same network discover, pair, and reconnect each other with no test harness gluing the pieces together. Verified by automated tests against real sockets and by manually running the built binary in this container; not yet verified across two genuinely separate physical machines (this environment only has the one). Bluetooth discovery/channels remain standalone, tested building blocks behind the `bluetooth` feature, not wired into `main.rs` — see `daemon/README.md`'s top status line for the exact split.
2. **Keyboard** — capture and transmit keyboard events. ◑ Real per-OS capture/injection (`flow-platform`, tracks E1/E4/E6) and a real streaming pipeline (track G8) exist and are wired into the daemon. The remaining gap is *local suppression*: capture is passive, so the sending machine must also stop its own OS processing the keystrokes it forwards. That's implemented on Linux (exclusive `EVIOCGRAB`) but **not on macOS or Windows**, where input currently reaches both machines at once. Only Linux capture/injection has run against real hardware in this project's own development environment (no macOS/Windows machine, no `/dev/input` in this container) — see "Local input suppression" and "Platform adapters" in `daemon/README.md`.
3. **Mouse** — add movement, clicks, and scroll. ✅ Same `InputEvent`/pipeline covers mouse alongside keyboard from the start (`core::protocol::MouseEvent`) — not a separate later addition in the Rust implementation.
4. **Switching** — implement Scroll Lock-based device switching. ✅ `daemon/todos.json` track F: a configurable switch-key binding (Scroll Lock is one of several presets), detected locally without needing the Flutter UI running.
5. **Flutter control plane** — build the first real UI (status, pairing, switch key, settings). ✅ Done in an earlier phase, and now the shipped app rather than a UI-only preview — see `flutter/README.md`: `flutter run` docks a real OS tray icon/window and talks to a real `flow-daemon` by default, with onboarding gating the dashboard on first launch. The original dev harness (every screen/state/platform combination without a daemon) is still there for manual QA, reachable via `--dart-define=FLOW_UI_MODE=harness`.
6. **Reliability** — stress-test network interruptions, restarts, and rapid switching. ◑ Structured logging and per-task panic isolation exist and are tested (`daemon/todos.json` track I); real peer connections recover on their own via discovery's periodic re-announce redialing a paired device once it's reachable again (not `channel::reconnect::maintain_connection`'s exponential-backoff loop, which remains a standalone, tested building block `main.rs` doesn't call directly — see `daemon/README.md`), with a deterministic tie-break so two daemons starting simultaneously don't drop each other's connections. Genuine multi-hour/rapid-switching stress testing against real hardware hasn't happened — this environment can't run two real daemons against each other continuously.
7. **Native daemon** — harden the POC into the production Rust daemon across macOS, Linux, and Windows. ✅ Every task in `daemon/todos.json` (tracks A-J) is done; see `daemon/README.md` for the full, honest account of what's verified how on each OS.

## Success criteria

**POC:** two computers connect reliably, keyboard/mouse events are transmitted with low perceptible latency, Scroll Lock switching feels instantaneous, and the system runs continuously without excessive resource usage. *Status: implemented and unit-tested end to end on the TCP medium, but never demonstrated on two physical machines, and local input suppression — without which input reaches both computers instead of just the active one — is Linux-only so far.*

**Product:** installation takes minutes, pairing is nearly effortless, switching is instant, the daemon is effectively invisible, the UI feels native on every OS, and users never have to think about which computer their peripherals are connected to.

## North star

One desk. Multiple computers. One keyboard. One mouse. No hardware switching, no extra peripherals, no messy desk, no complicated setup — just press a key and continue working.
