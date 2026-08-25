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

1. **Connectivity** — establish a reliable connection between two daemons.
2. **Keyboard** — capture and transmit keyboard events.
3. **Mouse** — add movement, clicks, and scroll.
4. **Switching** — implement Scroll Lock-based device switching.
5. **Flutter control plane** — build the first real UI (status, pairing, switch key, settings).
6. **Reliability** — stress-test network interruptions, restarts, and rapid switching.
7. **Native daemon** — harden the POC into the production Rust daemon across macOS, Linux, and Windows.

## Success criteria

**POC:** two computers connect reliably, keyboard/mouse events are transmitted with low perceptible latency, Scroll Lock switching feels instantaneous, and the system runs continuously without excessive resource usage.

**Product:** installation takes minutes, pairing is nearly effortless, switching is instant, the daemon is effectively invisible, the UI feels native on every OS, and users never have to think about which computer their peripherals are connected to.

## North star

One desk. Multiple computers. One keyboard. One mouse. No hardware switching, no extra peripherals, no messy desk, no complicated setup — just press a key and continue working.
