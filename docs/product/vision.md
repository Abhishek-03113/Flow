# Cross-Device Keyboard & Mouse

## Product Vision, Architecture & POC

Working concept: Seamlessly use one keyboard and mouse across multiple computers, switching the active computer with a dedicated keyboard shortcut — without purchasing specialized multi-device peripherals.

---

## 1. Problem

Power users and developers often work across multiple computers simultaneously.

A typical setup might involve:

- A MacBook for development or personal work
- A Windows/Linux machine for work, experimentation, or specialized workloads
- Two keyboards
- Two mice
- Multiple cables and receivers
- Limited desk space

While commercial keyboards and mice increasingly support multi-device switching, they require purchasing new hardware.

The goal of this project is to recreate the core multi-device peripheral experience entirely through software, allowing users to reuse the keyboard and mouse they already own.

### Core problem

How can I seamlessly control two or more computers using one keyboard and mouse without physically switching peripherals or buying specialized hardware?

---

## 2. Solution

Build a lightweight cross-platform utility that allows multiple computers to share a single keyboard and mouse.

Each computer runs a small background daemon responsible for:

- Capturing keyboard events
- Capturing mouse events
- Sending input events to another computer
- Receiving remote input events
- Injecting remote input locally
- Managing the active device
- Managing device pairing and trust
- Maintaining connectivity

The user can switch the active computer using a configurable keyboard shortcut.

### Initial switch key

Scroll Lock

Scroll Lock is a good starting point because it is relatively uncommon in modern workflows and therefore unlikely to interfere with normal keyboard usage.

Example:

```
                    ┌───────────────────┐
                    │     Keyboard      │
                    └─────────┬─────────┘
                              │
                              ▼
                    ┌───────────────────┐
                    │     Device A      │
                    │      Daemon       │
                    └─────────┬─────────┘
                              │
                    Scroll Lock toggles
                              │
               ┌──────────────┴──────────────┐
               │                             │
               ▼                             ▼
        ┌─────────────┐               ┌─────────────┐
        │   Device A  │◄─────────────►│   Device B  │
        │    Active   │    Channel    │   Inactive  │
        └─────────────┘               └─────────────┘
```

Pressing Scroll Lock switches the destination of keyboard and mouse input.

---

## 3. Product Vision

The long-term goal is not simply to build a keyboard-sharing daemon.

The goal is to make multiple computers feel like they are part of the same physical workspace.

The experience should feel:

- Instant
- Invisible
- Reliable
- Lightweight
- Cross-platform
- Native
- Configurable
- Polished

The product should aim for an Apple-level seamless experience:

The computers should feel like they already know they belong together.

The networking, device pairing, event routing, and daemon complexity should remain invisible to the user.

---

## 4. Product Principles

1. **Invisible by default**

   The user should forget the software is running.

2. **Instant interaction**

   Switching computers should feel immediate.

3. **Native system integration**

   The product should behave like an OS utility rather than a traditional application.

4. **Zero physical clutter**

   The product exists to reduce desk clutter, so the software UX should follow the same principle.

5. **Cross-platform first**

   macOS, Windows, and Linux should all be first-class platforms.

6. **Progressive complexity**

   Simple users should see a simple interface.

   Power users should have access to deeper networking, input, and debugging configuration.

7. **UI and system functionality are separate**

   The graphical interface should not be responsible for the core input-sharing functionality.

   The daemon should continue working even if the UI is closed.

---

## 5. High-Level Architecture

The architecture should be split into two major layers:

```
┌───────────────────────────────────────────┐
│                Flutter UI                 │
│                                           │
│  Menu Bar / System Tray                   │
│  Device Management                        │
│  Pairing                                  │
│  Settings                                 │
│  Connection Status                        │
│  Onboarding                               │
└─────────────────────┬─────────────────────┘
                      │
                 Control Link
                      │
┌─────────────────────▼─────────────────────┐
│             Native Daemon                 │
│                  Rust                     │
│                                           │
│  Input Capture                            │
│  Input Injection                          │
│  Device Switching                         │
│  Networking                               │
│  Pairing                                  │
│  Security                                 │
│  Configuration                            │
│  Device State                              │
└─────────────────────┬─────────────────────┘
                      │
                 Channel Layer
                      │
          ┌───────────┼───────────┐
          ▼           ▼           ▼
      WebSocket    Bluetooth     Future
```

This separation is one of the core architectural decisions of the project.

---

## 6. Why Flutter + Rust?

A major goal is to avoid maintaining three completely different user interfaces for macOS, Linux, and Windows.

Flutter provides a strong cross-platform UI layer while Rust handles the parts of the system where platform-specific behavior is unavoidable.

### Flutter owns

- Menu bar UI
- System tray UI
- Settings
- Device management
- Pairing UI
- Onboarding
- Connection status
- Notifications
- Configuration

### Rust owns

- Global keyboard capture
- Mouse capture
- Input injection
- Global hotkeys
- Scroll Lock detection
- WebSocket/networking
- Device discovery
- Pairing
- Encryption
- Background execution
- OS-specific integrations

The conceptual boundary is:

Flutter = what the user sees

Rust = what the computer actually does

---

## 7. Flutter Does Not Remove OS Complexity

Flutter significantly reduces UI fragmentation, but it does not eliminate platform-specific system work.

Keyboard and mouse capture/injection are inherently OS-dependent.

The architecture should therefore isolate platform-specific functionality behind a common interface.

```
                     Core Daemon
                          │
             ┌────────────┼────────────┐
             │            │            │
             ▼            ▼            ▼
          macOS        Windows       Linux
          Adapter      Adapter       Adapter
             │            │            │
             ▼            ▼            ▼
        Native APIs   Native APIs   Native APIs
```

The rest of the daemon should not need to know how each operating system implements input capture.

For example:

```
InputCapture
├── macOSInputCapture
├── WindowsInputCapture
└── LinuxInputCapture
```

and:

```
InputInjector
├── macOSInputInjector
├── WindowsInputInjector
└── LinuxInputInjector
```

This keeps the OS-specific complexity contained.

---

## 8. Why Not Put Everything in Flutter?

Flutter should not become the actual keyboard/mouse-sharing engine.

The final system should behave like a system utility:

```
Computer boots
      │
      ▼
Daemon starts
      │
      ▼
Device connection established
      │
      ▼
Input sharing works
```

The Flutter UI should be optional:

```
Daemon
  │
  ├── Works without UI
  │
  └── Flutter UI
        │
        └── Controls / observes daemon
```

This provides several benefits:

- Lower resource usage
- Better reliability
- UI crashes don't stop input sharing
- Daemon can start at boot
- Easier background operation
- Cleaner architecture
- Channel can evolve independently
- UI can be redesigned without touching core functionality

---

## 9. Communication Between Flutter and Daemon

Flutter should communicate with the local daemon through a Control Link rather than directly managing remote device networking.

Conceptually:

```
Flutter
   │
   │ Control Link
   ▼
Daemon
   │
   │ Channel
   ▼
Other Device
```

Possible Control Link mediums:

- Unix domain sockets on macOS/Linux
- Named pipes on Windows
- Local TCP as a cross-platform fallback
- Other platform-specific local mediums

The exact medium can be decided during implementation.

The important architectural rule is:

Flutter should not care whether remote devices communicate over WebSocket, Bluetooth, QUIC, or another future Channel medium.

---

## 10. Connectivity

The Channel abstraction should be in place from the beginning.

### POC Channel

The initial implementation should use:

WebSocket over the local network

```
Device A Daemon
       │
       │ WebSocket
       ▼
   Local Network
       │
       ▼
Device B Daemon
```

This provides:

- Simple implementation
- Easy debugging
- Cross-platform support
- Low latency
- Straightforward event streaming
- Easy event inspection during development

---

### Future Channels

The architecture should allow additional Channel implementations.

Potential options:

**WebSocket**

Initial LAN Channel medium.

**Bluetooth**

Useful when devices are nearby and the user does not want to depend on the local network.

**TCP / QUIC**

Potential future low-level Channel mediums.

**Other local Channel mediums**

The Channel abstraction should remain replaceable without changing the input or application layers.

Conceptually:

```
                  Event Protocol
                       │
          ┌────────────┼────────────┐
          │            │            │
          ▼            ▼            ▼
      WebSocket    Bluetooth      QUIC
```

---

## 11. Input Event Protocol

Keyboard and mouse events should be represented using a platform-independent event model.

### Keyboard

```
KeyboardEvent
├── KeyDown
├── KeyUp
└── ModifierState
```

### Mouse

```
MouseEvent
├── Move
├── ButtonDown
├── ButtonUp
└── Scroll
```

Example:

```json
{
  "type": "keyboard",
  "event": "keydown",
  "key": "A",
  "modifiers": ["SHIFT"],
  "timestamp": 123456789
}
```

Mouse:

```json
{
  "type": "mouse",
  "event": "move",
  "dx": 12,
  "dy": -4,
  "timestamp": 123456790
}
```

The event protocol should remain independent of:

- Operating system
- Channel medium
- UI
- Device topology

---

## 12. Input Switching

Switching is the defining interaction of the product.

### Default

Scroll Lock

### Future configurable options

Users should eventually be able to choose:

- Scroll Lock
- Pause/Break
- Function keys
- Custom key combinations
- Modifier combinations
- Other supported global shortcuts

Example:

```
Switch Device
Current shortcut:
[ Scroll Lock ▼ ]
Alternative:
[ Ctrl + Shift + Space ]
[ F13 ]
[ Pause ]
[ Custom... ]
```

---

## 13. Device State

The system should maintain a clear state model.

Possible states:

```
PAIRING
CONNECTED
ACTIVE
INACTIVE
DISCONNECTED
ERROR
```

Example:

```
Device A
──────────────
● Connected
● Active
Device B
──────────────
● Connected
○ Inactive
```

The active state should be synchronized between connected devices.

---

## 14. System UI

The application should primarily live inside the operating system's native system UI.

It should not require a traditional application window during normal operation.

### macOS

The application should live in the menu bar.

Example:

```
┌───────────────────────────────┐
│ Cross Device                  │
│                               │
│ ● Connected                   │
│                               │
│ Active Device                 │
│   ● MacBook                   │
│   ○ Work Laptop               │
│                               │
│ ───────────────────────────── │
│                               │
│ Switch Key                    │
│   Scroll Lock                 │
│                               │
│ Pair Device...                │
│ Settings...                   │
│ Quit                          │
└───────────────────────────────┘
```

### Linux

The application should integrate with the desktop environment's system tray/status area where supported.

The implementation should account for differences between:

- GNOME
- KDE Plasma
- Other Linux desktop environments

### Windows

The application should live in the Windows system tray.

```
┌───────────────────────────────┐
│ Cross Device                  │
│                               │
│ ● Connected                   │
│ Active: Work Laptop           │
│                               │
│ Switch Device                 │
│ Pair Device...                │
│ Settings                      │
└───────────────────────────────┘
```

---

## 15. Settings

The settings experience should remain simple while allowing progressive expansion.

### Connection

- Paired devices
- Pair new device
- Remove device
- Connection status
- Preferred Channel medium
- Auto reconnect

### Input

- Switch key
- Keyboard sharing
- Mouse sharing
- Scroll behavior
- Mouse sensitivity
- Modifier handling

### Startup

- Start on system boot
- Run in background
- Auto-connect paired devices

### Advanced

Potential future settings:

- Channel configuration
- Debug logging
- Event latency
- Connection timeout
- Custom event mappings
- Network diagnostics

---

## 16. Pairing

Pairing should be extremely simple.

On Device A:

```
Available Devices
┌──────────────────────────────┐
│ Work Laptop                  │
│ 192.168.x.x                  │
│                              │
│ [ Pair ]                     │
└──────────────────────────────┘
```

On Device B:

```
Pairing Request
"MacBook" wants to connect.
[ Reject ]     [ Accept ]
```

Once accepted, the devices become trusted.

Future versions should use secure device identity and key exchange rather than relying purely on local-network discovery.

---

## 17. Security

Keyboard and mouse events are highly sensitive.

Even though the initial POC can assume a trusted local network, the production product must treat the Channel as sensitive.

The eventual system should support:

- Device authentication
- Secure pairing
- Encrypted communication
- Trusted-device identities
- Replay protection
- Connection authorization
- Automatic rejection of unknown devices

Conceptually:

```
Device A
   │
   │ Secure Pairing
   ▼
Device Identity
   │
   ▼
Encrypted Channel
   │
   ▼
Input Events
```

Security should be part of the architecture even if it is simplified during the POC.

---

## 18. Technology Stack

### Production

**Core daemon**

Rust

Primary responsibilities:

- Input capture
- Input injection
- Networking
- Device management
- Pairing
- Security
- Configuration
- Background execution

**UI**

Flutter

Responsibilities:

- macOS menu bar UI
- Windows system tray UI
- Linux system tray UI
- Settings
- Pairing
- Device management
- Onboarding
- Status

**Communication**

```
Flutter
   ↕
Control Link
   ↕
Rust Daemon
   ↕
Channel
   ↕
Other Daemon
```

**Initial remote Channel medium**

WebSocket

**Future**

- Bluetooth
- QUIC
- Other low-latency Channel mediums

---

## 19. Repository Architecture

A potential project structure:

```
cross-device/
│
├── core/
│   ├── protocol/
│   ├── device/
│   ├── pairing/
│   ├── channel/
│   └── state/
│
├── daemon/
│   └── main.rs
│
├── platform/
│   ├── macos/
│   ├── windows/
│   └── linux/
│
├── flutter/
│   ├── onboarding/
│   ├── devices/
│   ├── settings/
│   ├── tray/
│   └── services/
│
└── installer/
    ├── macos/
    ├── windows/
    └── linux/
```

The exact repository structure can evolve, but the architectural separation should remain.

---

## 20. POC Strategy

The most important principle for the first version:

Validate the experience before building all of the platform complexity.

The first version should not immediately attempt to solve:

- Production daemon installation
- Native system services
- Bluetooth
- Production-grade pairing
- Complete security architecture
- Auto-start installers
- Perfect native system-tray integration

Instead, prove the fundamental interaction.

Can one keyboard and mouse reliably control two computers, and can a single key instantly switch the active destination?

---

## 21. POC Architecture

The POC should still validate the future architecture rather than becoming a throwaway prototype.

```
                 POC
                  │
          ┌───────┴────────┐
          │                │
      Flutter UI       Rust Daemon
                           │
                        WebSocket
                           │
                           ▼
                      Other Daemon
```

The daemon can initially expose a simple local API to Flutter.

Flutter controls:

- Pairing
- Connection
- Active device
- Switch key
- Debug information

The daemon handles:

- Event capture
- Event transmission
- Event reception
- Input injection
- Device state

This means the POC can evolve directly into the production architecture.

---

## 22. POC Milestones

### Phase 1 — Connectivity

Establish communication between two machines.

```
Daemon A
   │
   │ WebSocket
   ▼
Daemon B
```

Validate:

- Device discovery
- Connection
- Disconnect
- Reconnect
- Basic messages

---

### Phase 2 — Keyboard

Capture keyboard events on Device A and transmit them to Device B.

Validate:

- KeyDown
- KeyUp
- Modifiers
- Key repeat
- Special keys

---

### Phase 3 — Mouse

Add:

- Mouse movement
- Left click
- Right click
- Middle click
- Scroll

---

### Phase 4 — Switching

Implement:

```
Scroll Lock
      ↓
Device A ←→ Device B
```

Only the active device should receive input.

---

### Phase 5 — Flutter Control Plane

Build the first real UI:

- Connection status
- Device name
- Active device
- Pairing
- Switch key configuration
- Basic settings

---

### Phase 6 — Reliability

Test:

- Network interruptions
- Device restart
- Daemon restart
- Duplicate events
- Event ordering
- High-frequency mouse movement
- Keyboard repeat
- Modifier keys
- Rapid device switching

---

### Phase 7 — Native Daemon

**Status: `daemon/todos.json`'s full build-out plan (tracks A-J) is done** — the production Rust daemon this phase describes exists, including several items §20's POC Strategy explicitly deferred (Bluetooth, production-grade pairing, a real security architecture, and service-supervision scaffolding). See `daemon/README.md`'s top status line for the honest, current account of what's verified how on each OS, and what real end-to-end wiring (an incoming-connection accept loop in `main.rs`, splicing the Noise/trust/reconnect building blocks into it) is still deliberately outstanding.

Once the POC validates the interaction:

```
Web POC
   │
   │ Validate
   ▼
Flutter + Rust Architecture
   │
   ▼
Native Daemon
   │
   ├── macOS
   ├── Linux
   └── Windows
```

The production version should primarily involve hardening and replacing platform-specific prototypes rather than redesigning the entire system.

---

## 23. Future Possibilities

Once reliable keyboard and mouse sharing works, the product could evolve into a broader cross-device utility.

Potential features:

- More than two computers
- Automatic device switching
- Mouse-position-based switching
- Clipboard synchronization
- Universal clipboard
- Text sharing
- File transfer
- Drag-and-drop file transfer
- Device discovery
- Cross-device notifications
- Remote shortcuts
- Per-device input profiles
- Application-aware switching
- Device presence detection
- Secure remote control

The longer-term concept could become:

```
                 ┌──────────────┐
                 │   MacBook    │
                 └──────┬───────┘
                        │
                 ┌──────▼───────┐
                 │ Cross Device │
                 └──────┬───────┘
                        │
             ┌──────────┼──────────┐
             │          │          │
             ▼          ▼          ▼
          Laptop      Desktop     Server
```

At that point, the product becomes more than keyboard sharing — it becomes a lightweight personal device orchestration layer.

---

## 24. Success Criteria

### POC success

The POC is successful if:

- Two computers establish a connection reliably.
- Keyboard events are transmitted with low perceptible latency.
- Mouse movement works reliably.
- Mouse clicks work reliably.
- Scroll events work correctly.
- Scroll Lock switches the active computer.
- Switching feels instantaneous.
- Disconnect/reconnect works.
- The system can run continuously without excessive resource usage.

### Product success

The production product is successful when:

- Installation takes only a few minutes.
- Pairing is nearly effortless.
- Switching is instant.
- The daemon is effectively invisible.
- The UI feels native on every supported OS.
- Connection failures recover automatically.
- Security does not compromise usability.
- Users never need to think about which computer their peripherals are connected to.

---

## 25. North Star

One desk. Multiple computers. One keyboard. One mouse.

No hardware switching.

No extra peripherals.

No messy desk.

No complicated setup.

Just press a key and continue working.

The computer you're controlling should feel like the computer you're sitting in front of.

---

## 26. Architectural North Star

The final product should maintain a simple conceptual model:

```
                    USER
                     │
                     ▼
              ┌──────────────┐
              │ Flutter UI   │
              │              │
              │ Configure    │
              │ Pair         │
              │ Observe      │
              └──────┬───────┘
                     │
              Control Link
                     │
                     ▼
              ┌──────────────┐
              │ Rust Daemon  │
              │              │
              │ Capture      │
              │ Route        │
              │ Inject       │
              │ Secure       │
              └──────┬───────┘
                     │
                  Channel
                     │
                     ▼
              ┌──────────────┐
              │ Other Device │
              │              │
              │ Rust Daemon  │
              └──────────────┘
```

The most important architectural rule is:

Flutter should be the control plane, not the data plane.

The daemon owns the real-time input pipeline. Flutter provides the polished human interface around it.

This gives the project a clean path from a quick WebSocket POC to a production-grade, cross-platform system utility without throwing away the fundamental architecture.
