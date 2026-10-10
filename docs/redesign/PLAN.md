# Flow 2.0 redesign — master plan

This is the single source of truth every implementation track works from. Read it fully before starting a track.
Architecture decisions live in `docs/adr/`; the control model is ADR 0001 (read it).

## Goals

1. **Full client port Flutter → Tauri 2** — one Rust ecosystem, the engine embedded in-process (no local IPC between UI and backend).
2. **Controller/Controlled control model** (deskflow-style) — fixes "can't regain control", "remote input flows back", keystroke leakage.
3. **Gap analysis UI ↔ engine** and closing every gap; UI follows Apple HIG and feels like a consumer product.
4. **Data-driven keymaps** — per-OS keymaps and user remaps in config files, no redeploy for a mapping change.
5. **Test harness** — N virtual daemons with virtual input devices, emulating every OS pair, to test every case without hardware.
6. **Platform engineering** — CI, lint/format gates, task runner, contributor docs, reproducible builds.

## Target repository layout

```
core/            flow-core       protocol v2, input model, keymap model, settings/layout model (no OS deps)
platform/        flow-platform   per-OS capture/injection + keymap *loading* (tables come from config/keymaps)
daemon/          flow-daemon     the engine as a LIBRARY (service, router, links, pairing, storage, control API)
                                 + a headless `flow-daemon` bin (servers, harness, Playwright E2E)
harness/         flow-harness    virtual platform + multi-daemon scenario runner + CLI + HTML report
app/             Tauri 2 desktop app
  src-tauri/     flow-app crate: embeds flow-daemon engine, tray, windows, Tauri commands/events
  ui/            React 18 + TypeScript + Vite frontend (HIG design system), Vitest + Playwright
config/keymaps/  windows.toml macos.toml linux.toml  (default keymaps, embedded at compile time)
docs/            adr/, redesign/, contracts/, progress/ (milestone screenshots + logs)
flutter/         DELETED at the end of the Tauri track
```

## Control API (one contract, two transports)

The engine exposes exactly one command dispatcher (`daemon/src/ipc/dispatch.rs` → evolve, don't duplicate) and one
event stream. Two thin transports reuse it:

- **Tauri** (production): a single `#[tauri::command] flow_command(command, payload)` → dispatcher; engine events
  forwarded with `app.emit("flow://event", {event, payload})`. No sockets.
- **WebSocket** (headless daemon, harness, browser dev mode, Playwright): the existing loopback WS server with token
  subprotocol. The frontend's `WebSocketTransport` speaks it so the real UI can be driven in headless Chromium against
  real (virtual-platform) daemons.

The frontend has one `FlowClient` with pluggable `Transport` (`tauri` | `websocket` | `mock`). Selection: Tauri
runtime detected → tauri; `?daemon=ws://127.0.0.1:PORT&token=...` → websocket; otherwise → mock (demo data).

### Contract 0.2.0 additions (all tracks code against these names)

Settings (`FlowSettings`) gains:
- `role`: `"controller" | "controlled"` (default `controller`)
- `return_home_key`: `SwitchKeyBinding | null`
- `edge_switching`: `bool` (default `true`), `edge_switch_delay_ms`: `u32` (default `0`)
- `keymap_overrides_path`: read-only string (where the user's keymap/remap files live)

New types:
- `ScreenLayout { screens: [{ device_id, x, y, width, height }] }` — a deskflow-style arrangement in a shared virtual
  coordinate space; adjacency is derived from touching edges. Persisted on the controller.
- `ControlState { role, active_screen: "local" | DeviceId, controller_device_id: DeviceId | null }`
- `Device` gains `os`, `role`, `screen {width,height}` (when known), `last_seen_ms`, `link_error` (e.g. `role_conflict`).
- `Diagnostics { switch_latency_ms_p50, switch_latency_ms_p95, events_forwarded, events_dropped, uptime_s }`.
- `RemapRule { from: KeyName, to: KeyName, when_target_os: "macos"|"windows"|"linux"|null }`.

New commands: `switch_to {target: "local"|DeviceId}`, `release_control` (controlled side), `set_screen_layout
{layout}`, `set_role {role}`, `get_keymap {os}` / `set_remap_rules {rules}`, `reveal_config_dir`, `export_diagnostics`.
`switch_active_device` stays as an alias of `switch_to` for compatibility during the transition.

New events: `control_state_changed`, `screen_layout_changed`, `diagnostics_changed`, `remap_rules_changed`
(sent on connect like the existing six).

The contract doc `docs/contracts/daemon-ipc.md` + `data-model.md` + `CHANGELOG.md` must be updated by whichever
track lands the engine side of each addition.

## Tracks, branches, sequencing

Branch naming: conventional `<type>/<scope>-<desc>` (`feat/`, `fix/`, `refactor/`, `test/`, `chore/`, `docs/`, `ci/`).
Each track works in its own git worktree under `/home/user/Flow/worktrees/<branch-leaf>` (gitignored), branched from
`claude/epic-dijkstra-6qhmav` (the integration branch). The orchestrator merges finished tracks into the integration
branch with `--no-ff`.

| Wave | Branch | Track |
|---|---|---|
| 1 | `feat/engine-controller-model` | A — control model v2, InputRouter, protocol v2, platform-backend seam, leak fixes |
| 1 | `refactor/keymap-config` | B — keymaps to `config/keymaps/*.toml`, user overrides, remap rules |
| 1 | `feat/tauri-ui` | C — gap analysis + Tauri scaffold + full HIG UI against mock/WS transports + Playwright screenshots |
| 2 | `test/harness-virtual-daemons` | D — flow-harness: virtual platform, multi-daemon scenarios, OS-pair matrix, report |
| 2 | `feat/tauri-engine-embed` | E — embed engine in Tauri, tray, contract 0.2 wiring end-to-end, delete Flutter |
| 2 | `ci/platform-engineering` | F — GitHub Actions, justfile, fmt/clippy/eslint/prettier gates, CONTRIBUTING, release packaging |
| 3 | `test/e2e-ui-against-daemons` | G — Playwright E2E: real UI ↔ harness daemons, journeys, screenshots |
| 3 | `fix/*` | H — review findings, polish loop until every goal is met |

## Engineering rules (every track)

- **SRP / OCP, no gratuitous layers.** Reuse existing modules (dispatcher, storage repos, Noise channel, discovery,
  pairing). Abstract only what has ≥2 real implementations (e.g. `PlatformBackend`: native vs virtual; `Transport`:
  tauri vs websocket vs mock). Don't wrap single-use functions.
- **Every milestone ends with**: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` green (and `npm run lint && npm run typecheck && npm test` for UI work), a conventional
  commit (`feat(engine): …`), and a screenshot in `docs/progress/<track>/NN-<milestone>.png` committed with it.
- **Screenshots**: UI milestones → Playwright + headless Chromium (`/opt/pw-browsers`, launched with
  `--remote-debugging-port=9222`, light and dark). Backend milestones → render the test/harness output (HTML report or a
  terminal-styled HTML page of the test run) and screenshot it the same way. Never `playwright install`.
- Do not leave stale docs: when behaviour changes, the README/contract that describes it changes in the same commit.
- Commit trailer (required on every commit):
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01VqG8KjxmVRiXuMMAxn8FU4
  ```
- Do not push and do not open PRs from a track; the orchestrator integrates and pushes.
