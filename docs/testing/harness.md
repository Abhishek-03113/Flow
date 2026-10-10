# flow-harness: virtual daemons on emulated operating systems

`flow-harness` (crate `harness/`) runs many complete Flow engines in one process, each on a
virtual operating system, and drives them with physical-looking input. It is how every
controller/controlled behaviour of [ADR 0001](../adr/0001-controller-controlled-input-model.md)
is checked on every OS pair without hardware. It has two modes:

- **run** (the default): execute the scenario catalogue across the OS matrix and write
  reports. CI runs this on every push.
- **up**: start a set of virtual devices with real control servers and keep them running, so
  the real UI (Playwright, track G) or a developer can drive them.

What it does not replace: the platform adapters themselves (Windows hooks, the macOS event
tap, evdev/uinput) are not exercised; that is what [`physical-test-script.md`](physical-test-script.md)
is for.

## Architecture

```
            one process, one tokio runtime
 ┌───────────────────────────────────────────────────────────┐
 │ World (one per case)                                      │
 │  ┌───────────── VirtualDevice "ctl" (windows) ─────────┐  │
 │  │ OsKeyboard(windows keymap)   harness feeds native   │  │
 │  │        │ capture adapter:    key codes -> wire names│  │
 │  │        ▼                                            │  │
 │  │ VirtualBackend ──► Engine (flow-daemon, unmodified) │  │
 │  │  records: delivered_locally, injected, warps,       │  │
 │  │           capturing, suppressed, cursor             │  │
 │  └──────────────────────────┬──────────────────────────┘  │
 │      real discovery (UDP, explicit loopback targets),     │
 │      real pairing + Noise, protocol v2 over loopback TCP  │
 │  ┌──────────────────────────┴──────────────────────────┐  │
 │  │ VirtualDevice "ctd" (macos) ... same shape          │  │
 │  └─────────────────────────────────────────────────────┘  │
 └───────────────────────────────────────────────────────────┘
```

| Module | Role |
|---|---|
| `harness/src/world.rs` | `World`: starts engines (temp data dir, ephemeral ports, discovery limited to explicit loopback targets, so worlds never see each other or the LAN), sets roles, pairs devices through the real discovery and pairing flow with a pretend UI that accepts consent, feeds physical input, disconnects/reconnects engines, and waits until the world is quiet (`settle`). |
| `harness/src/keys.rs` | `OsKeyboard`: per-OS keyboard emulation from the embedded keymaps. |
| `harness/src/events.rs` | The event notation used in scenarios and how observed events are compared. |
| `harness/src/scenario.rs` | The scenario data model (TOML), the OS matrix, loading and filtering. |
| `harness/src/runner.rs` | Runs one scenario on one OS combination and records a per-step timeline. Knows nothing about particular scenarios. |
| `harness/src/matrix.rs` | Runs scenarios x combinations concurrently (`--jobs`, default 12 worlds at once). |
| `harness/src/report.rs` | `report.html`, `report.json`, `junit.xml`. |
| `harness/src/up.rs` | `up` mode and its manifest. |
| `daemon/src/virtual_backend.rs` | The `PlatformBackend` with no OS behind it (shared with `daemon/tests/two_engines.rs`). |

The engines are the production `flow_daemon::engine::Engine`, not a model of it: router,
links, pairing, storage, settings and the control API are the real code. The only
substitution is the `PlatformBackend`.

### How OS emulation works

A real capture adapter receives a **native key code** of its OS and turns it into a wire
key name with that OS's keymap; the injector on the other side turns the wire name back into
*its* OS's native code. The harness does the same with the real tables in
`config/keymaps/{windows,macos,linux}.toml` (embedded, via `flow_core::keymap::Keymap::embedded`):

1. `press A on ctl` looks up the native code of `A` on `ctl`'s OS (`0x41` on Windows, `0x00`
   on macOS), then reports it the way the adapter would (`name_for_code`). A key the OS does
   not have (for example `SCROLLLOCK` on a Mac keyboard) is a step error, so scenarios cannot
   type keys a real keyboard lacks.
2. Expectations such as `"down A"` are resolved on the **observing** device's OS and compared
   by native code, so one scenario checks the cross-OS translation in every cell of the matrix.
   Reports print both: `down A [0x00]`.
3. Each device has its own screen size and a pointer position (used by edge switching). A
   controller without Scroll Lock (macOS) is given the F13 switch-key preset, as a user would.

The `VirtualBackend` records two logs per device: **delivered locally** (physical events the
router let through to the device's own applications, `Disposition::Pass`) and **injected**
(events the engine injected as a controlled device). Most checks are statements about these
two logs plus the engines' own state (active screen, suppression, `entered`, links).

### Timing: no sleeps

Every wait polls a condition with a deadline (`world::wait_until`). `expect` first waits for
the world to be **quiet** (nothing observable changed for 50 ms: event counts, warps, active
screen, `entered`), so absence ("nothing leaked") can be asserted without fixed sleeps, then
re-checks until it holds or the scenario timeout (default 10 s) runs out. Hotkey debounce is
0 by default so scenarios can switch faster than a human.

## Running

```sh
cargo run -p flow-harness --release -- --report-dir target/harness-report   # what CI runs
./scripts/dev.sh harness --report-dir target/harness-report                 # same, via the task runner
cargo test -p flow-harness                                                  # same catalogue as tests
```

```
flow-harness [run] [--scenario GLOB] [--pairs all|win-mac,...] [--report-dir DIR]
                   [--jobs N] [--scenarios-dir DIR]
```

| Flag | Meaning |
|---|---|
| `--scenario GLOB` | Only scenarios whose `name` matches (`*`, `?`), e.g. `--scenario 'held-*'`. |
| `--pairs LIST` | Only these matrix cells, e.g. `--pairs win-mac,mac-win` (or `all`). Triples are named the same way: `win-mac-lin`. |
| `--report-dir DIR` | Write `report.html`, `report.json` and `junit.xml` there. |
| `--jobs N` | Cases run at once (default 12). |
| `--scenarios-dir DIR` | Load scenarios from another directory instead of `harness/scenarios/`. |

The exit code is 0 when every case passed, 1 otherwise, 2 for a usage error. Progress goes to
stderr (`ok     basic-switch [win-mac] 2258 ms`); failures are printed in full at the end with
the failing step, expected and actual. The full catalogue runs in about 30 s.

`cargo test -p flow-harness` runs the same catalogue: `harness/build.rs` generates one test
per scenario file (`harness/tests/catalogue.rs`), so a new scenario is a test the moment it
is added and `cargo test -p flow-harness rapid` runs one.

### Reports

- **`report.html`**: verdict and totals, a list of failures (each with the failing step,
  expected vs actual, and the exact command to rerun just that case), the matrix (one table for
  OS pairs and one for triples; rows are scenarios, columns are `controller → controlled` OS
  combinations), and per scenario its description and one collapsible step timeline per case.
  The first failing case of each scenario opens by default. Light and dark follow the system,
  with a toggle.
- **`report.json`**: the same data (`scenarios[].cases[].steps[]` with `status`, `duration_ms`,
  `expected`, `actual`, `message`) for tooling.
- **`junit.xml`**: one `testsuite` per scenario, one `testcase` per OS combination, for CI test
  summaries.

CI (`.github/workflows/ci.yml`, job `harness`) runs the matrix and uploads
`target/harness-report/` as the `harness-report` artifact.

## Scenario DSL reference

A scenario is one TOML file in `harness/scenarios/`. Unknown keys are errors, so a typo fails
loudly instead of being ignored.

```toml
name = "basic-switch"                 # unique; used by --scenario and the report
description = """What it proves."""
matrix = "pairs"                      # "pairs" (default): all 9 controller x controlled OS pairs
                                      # "triples": win-mac-lin, mac-lin-win, lin-win-mac, win-win-mac
pairs = ["win-mac", "mac-win"]        # optional: restrict to these cells

[world]                               # all optional
security = "secure"                   # "secure" (default: Noise + pairing consent) | "insecure"
debounce_ms = 0                       # switch hotkey debounce (default 0)
timeout_ms = 10000                    # deadline for every wait/expect (default 10000)

[[devices]]                           # default: ctl = controller on $a, ctd = controlled on $b
name = "ctl"
os = "$a"                             # $a, $b, $c = the matrix cell's OSes; or windows|macos|linux (win|mac|lin)
role = "controller"                   # "controller" | "controlled"
screen = [1920, 1080]                 # default 1920x1080

[[links]]                             # default: every controller linked to every controlled device
a = "ctl"                             # the side that starts pairing
b = "ctd"
expect = "live"                       # "live" | "refused:<link_error>"; default: live, or
                                      # refused:role_conflict when both have the same role

[[steps]]
do = "type"
on = "ctl"
text = "ab"
```

Every step may carry `when = "ctd=macos"` or `when = "ctd!=macos"` (device name and OS); a
step whose guard is false is reported as skipped.

### Steps (`do = ...`)

| `do` | Fields | What it does |
|---|---|---|
| `press` / `release` | `on`, `key` | Press or release one physical key, by wire key name (`A`, `SHIFT`, `F13`, `RIGHT_COMMAND`, see `core/src/protocol/key_names.rs`). Modifier state is tracked and sent with each key. |
| `type` | `on`, `text` | Press and release each character (letters, digits, space). |
| `move` | `on`, `dx`, `dy`, `times` = 1 | Relative pointer motion. While the device's own screen is active its virtual pointer follows (clamped to the screen), which is what edge switching watches. |
| `button_down` / `button_up` / `click` | `on`, `button` = `left` | `left`, `right`, `middle`. |
| `scroll` | `on`, `dx`, `dy` | One wheel event. |
| `place_cursor` | `on`, `x`, `y` | Put the device's own pointer at (x, y), as if dragged there. |
| `hotkey` | `on`, `which` = `switch` | Press the configured switch chord (all keys down, then up in reverse); `which = "return_home"` presses the return-home chord. |
| `switch_to` | `on`, `target`, `expect_error`? | Control API `switch_to` on a controller; `target` is a device name or `local`. With `expect_error` the command must be refused with that code. |
| `release_request` | `on`, `expect_error`? | Control API `release_control` on a controlled device. |
| `set_setting` | `on`, `key`, `value` | `update_settings` with one field, e.g. `key = "share_mouse", value = false`, `pointer_sensitivity`, `edge_switching`, `return_home_key`. An unknown key is an error. |
| `set_remaps` | `on`, `rules = [{ from, to, when_target_os? }]` | `set_remap_rules`. |
| `set_layout` | `on`, `screens = [{ device, x, y }]` | `set_screen_layout` on the controller; `device = "local"` is the controller itself; sizes come from the devices' screens. |
| `disconnect` | `device` | Kill the engine abruptly (sockets close, no goodbye). |
| `reconnect` | `device` | Restart it on its old data dir (still paired) and wait for its links to come back. |
| `clear` | `on`? | Wait for quiet, then forget the recorded input logs (of one device, or all). Starts a fresh observation window. |
| `repeat` | `times`, `steps = [...]` | Run nested steps (inline tables) `times` times. |
| `wait` | a check | Poll until the check holds (no quiet wait first). Use for things that happen asynchronously, like an edge switch. |
| `expect` | a check | Wait for quiet, then require the check (re-checked until the timeout). |

### Checks (`that = ...`, for `wait` and `expect`)

Event lists use this notation: `down A`, `up A`, `move 10 -5`, `button_down left`,
`button_up right`, `scroll 0 -3`. `kinds = ["key", "move", "button", "scroll"]` restricts a
check to some event families.

| `that` | Fields | Holds when |
|---|---|---|
| `active` | `on`, `is` | The controller's active screen is `local` or the named device. |
| `injected` | `on`, `events`, `repeat` = 1, `kinds`? | Exactly these events (repeated `repeat` times) were injected into `on` since the last `clear`. |
| `injected_includes` | `on`, `events`, `kinds`? | These events appear consecutively, in order, among those injected. |
| `pointer_moved` | `on`, `dx`, `dy`, `tolerance` = 0 | The injected pointer motion sums to (dx, dy), per axis within `tolerance`. |
| `delivered` | `on`, `events`, `repeat` = 1, `kinds`? | Exactly these events reached the device's own applications. |
| `no_local_leak` | `on`, `kinds`? | Nothing (of these kinds) reached the device's own applications. |
| `no_forwarding` | `on` | No physical event fed to `on` was injected into any other device (traced by timestamp). |
| `key_never_seen` | `key` | The key was neither injected into nor delivered on any device (for hotkey chords). |
| `nothing_stuck` | `on` | Every key and button injected down on `on` was released again. |
| `suppressed` / `capturing` | `on`, `is` | The virtual OS's suppression / capture state. |
| `entered` | `on`, `is` | Controlled side: a controller is driving this machine. |
| `linked` | `on`, `peer`, `is` | `on` has a live link to `peer`. |
| `link_error` | `on`, `peer`, `is` | `on`'s `link_error` for `peer` (`role_conflict`, ..., or `none`). |
| `warped` | `on`, `x`, `y` | The last cursor warp on `on` was to (x, y). |

### Adding a scenario

1. Copy the closest file in `harness/scenarios/`, give it the next number and a new `name`.
2. Keep it to one behaviour; write the description as the claim it proves.
3. Use `clear` before each observation window and `expect` (not `wait`) when asserting absence.
4. Run it: `cargo run -p flow-harness --release -- --scenario my-scenario`, or
   `cargo test -p flow-harness my_scenario`.
5. If it finds an engine bug, fix the engine in its own `fix(engine): ...` commit and keep the
   scenario as the regression test.

## The catalogue

| Scenario | Matrix | Proves (ADR 0001) |
|---|---|---|
| `basic-switch` | pairs | Hotkey to the controlled machine, type there, regain control with the controller's own keyboard (the original Windows → Mac bug). |
| `controlled-input-never-forwarded` | pairs | A controlled machine's own keyboard and mouse stay local and never reach another device. |
| `no-local-leak-while-remote` | pairs | While remote, keys, moves, buttons and scroll never reach the controller's own apps. |
| `held-key-across-switch` | pairs | Up follows down for keys; keys held at a switch are released on the side that saw the press; autorepeat. |
| `held-button-across-switch` | pairs | Up follows down for mouse buttons (drag across a switch). |
| `hotkey-chord-never-reaches-an-os` | pairs | Multi-key chords are consumed, down and up, everywhere. |
| `return-home-key` | pairs | The return-home key jumps to local from any screen. |
| `release-request` | pairs | A controlled user's "Release control" returns the controller home; only a controlled device can release, only a controller can switch. |
| `disconnect-controlled-while-remote` | pairs | Link loss while remote returns home and un-suppresses; reconnect, switching works again. |
| `disconnect-controller-while-remote` | pairs | The controlled side releases held input when its controller vanishes; reconnect. |
| `role-conflict-controllers` / `role-conflict-controlled` | pairs | Same-role links are refused with `role_conflict`. |
| `rapid-switching` | pairs | 200 hotkey toggles with typing in between: everything lands on the right side, in order, nothing stuck. |
| `hotkey-autorepeat-debounce` | pairs | Holding the switch key does not toggle repeatedly. |
| `three-device-cycle` | triples | The hotkey cycles `Local → A → B → Local`, each screen gets exactly what was typed while active; `switch_to` jumps anywhere. |
| `edge-switching-layout` | triples | Edge crossings across a three-screen default layout and back, with the local pointer warped back. |
| `custom-layout` | pairs | A user layout (larger screen above the controller): only touching edges switch, entry points map both ways. |
| `remap-per-target-os` | pairs | Remap rules apply per target OS. |
| `sharing-settings` | pairs | `share_keyboard` / `share_mouse` off keep that input local. |
| `pointer-sensitivity` | pairs | `pointer_sensitivity` scales forwarded motion. |
| `edge-switching-off` | pairs | With `edge_switching = false` edges never switch; hotkey and `switch_to` still do. |

Heartbeat silence (a peer that stops talking without closing the socket) is not a harness
scenario: closing an engine always closes its sockets. It is covered by
`daemon/tests/link_roles.rs::heartbeat_silence_ends_the_link_and_releases_held_keys`.

## up mode (for UI E2E)

```sh
cargo run -p flow-harness --release -- up \
  --devices Mac:mac:controller,PC:win:controlled,Linux:lin:controlled \
  --paired --base-port 47900 --manifest target/up-manifest.json
```

| Flag | Meaning |
|---|---|
| `--devices LIST` | Required. `Name:os:role,...`; `os` is `win`/`mac`/`lin` (or `windows`/`macos`/`linux`), `role` is `controller` or `controlled`. Screens are 1920x1080. |
| `--paired` | Pair and link every controller with every controlled device before returning (consent auto-accepted during setup only). Without it the devices only discover each other and pairing is left to the UI. |
| `--base-port N` | Device `i` (in `--devices` order) serves its control API on `127.0.0.1:N+i`. Default: ephemeral ports. |
| `--manifest FILE` | Also write the manifest as a JSON array. The file appears once every device is up, so it doubles as the ready signal. |
| `--security secure\|insecure` | Peer security (default `insecure`, like local development). |

Each device prints one JSON line on stdout, then `devices are up; Ctrl-C to stop` on stderr.
The manifest (file) is an array of the same objects:

```json
[
  { "name": "Mac", "os": "macos",   "role": "controller", "ws_url": "ws://127.0.0.1:47900", "token": "9707…" },
  { "name": "PC",  "os": "windows", "role": "controlled", "ws_url": "ws://127.0.0.1:47901", "token": "6424…" }
]
```

Each engine runs with test hooks enabled. Ctrl-C (SIGINT) stops them all.

### Connecting

The control server is the daemon's normal loopback WebSocket
(`daemon/src/ipc/server.rs`, contract `docs/contracts/daemon-ipc.md`). Authentication is the
token sent as the WebSocket **subprotocol** (`Sec-WebSocket-Protocol: <token>`), which is what
a browser does with `new WebSocket(ws_url, token)`; the server echoes it back. A missing or wrong
token is refused with HTTP 401 before the upgrade. The real UI connects with
`?daemon=<ws_url>&token=<token>`.

Requests are `{"id": "1", "command": "...", "payload": ...}`; replies are `{"id", "ok": true}`,
`{"id", "ok": true, "result": ...}` or `{"id", "ok": false, "error": {"code", "message"}}`.
On connect the server sends the state snapshot as events (`{"event": "devices_changed",
"payload": [...]}`, ...), then every change.

### Debug commands (test hooks only)

Both are refused as `unknown_command` on an engine without test hooks.

| Command | Payload | Effect |
|---|---|---|
| `debug_inject_input` | An `InputEvent` in its wire JSON, e.g. `{"Keyboard":{"KeyDown":{"key":"A","modifiers":[],"timestamp_ms":1}}}`, `{"Mouse":{"Move":{"dx":10,"dy":0,"timestamp_ms":2}}}`, `{"Mouse":{"ButtonDown":{"button":"Left","timestamp_ms":3}}}` | Feeds the event to the device's virtual OS as a physical event (wire key names, no keymap step), so the router handles it exactly as captured input: forwarded when remote, delivered locally when home, hotkeys matched. |
| `debug_virtual_io` | `null` or `{"clear": true}` | Returns `{capturing, suppressed, delivered_locally: [InputEvent], injected: [InputEvent], warps: [[x, y]]}` for the device's virtual OS; `clear` empties the logs after answering. |

A round trip, as `harness/tests/up_mode.rs` does it automatically:

1. On the controlled device: `debug_virtual_io {"clear": true}`.
2. On the controller: read the controlled device's `id` from `devices_changed`, then
   `switch_to {"target": "<id>"}`.
3. On the controller: `debug_inject_input` a `KeyDown`/`KeyUp` pair.
4. On the controlled device: `debug_virtual_io` lists the events under `injected`; the
   controller's own `debug_virtual_io` shows `suppressed: true` and nothing in
   `delivered_locally`.
