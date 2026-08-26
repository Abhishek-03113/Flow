# Flow — code review

Reviewed at `main` (`2be87a6`, "Merge pull request #4 from
Abhishek-03113/test/e2e-daemon-ui-flow"). Covers the whole tree: the
three Rust crates (`core/`, `daemon/`, `platform/` — ~10.8k lines), the
Flutter control plane (`flutter/lib`, `flutter/test` — ~7k lines), the
contract and product docs, and the packaging scaffolding.

## Method

Every source file was read, not sampled. Claims below that say
"confirmed" were confirmed by running code, not by reading it:

| Check | Result |
|---|---|
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `cargo test --workspace` | 169 passing, 0 ignored on this target |
| `flutter analyze` | "No issues found" |
| `flutter test` | 59 passing, 2 suites skipped (`manual`, `e2e` tags) |

Two findings ([C1](#c1), [C2](#c2)) were reproduced with throwaway
integration tests before being written up; both now have permanent
regression tests in the tree (see [Refactoring applied](#refactoring-applied)).

All line references below are against the reviewed commit, before the
changes in [Refactoring applied](#refactoring-applied) shifted them.

## Overall

This is unusually disciplined work for its stage. The layering is real
and enforced, not aspirational: `flow-core` genuinely has no OS,
transport, or UI dependency; `tokio-tungstenite` really is confined to
`ipc::server` and `channel::tcp`; `rusqlite` really does appear in
exactly one module; the `Channel` trait really is the only thing pairing,
streaming and encryption are written against, with `negotiate.rs` as the
single place that knows concrete media exist. Test coverage is
proportionate and the tests assert behaviour rather than restating the
implementation. Comment quality is far above average — most non-obvious
decisions carry a comment saying *why*, and `daemon/README.md` is
unusually honest about what is and isn't wired up.

The problems are concentrated in one place, and it is the same place in
both languages: **state that lives only in memory, and code paths that
are only ever reached from tests.**

Three themes:

1. **The daemon is a one-boot system.** `DeviceState` is deliberately not
   persisted, and nothing restores it on load, so every boot after the
   first leaves *every* device — including this machine — `Disconnected`,
   with no device in a switchable state and the switch key permanently
   inert. Confirmed by execution ([C1](#c1)).

2. **The data plane is not reachable from `main.rs`.** It starts the
   database, the service, the history logger, the hotkey runner, the
   log-level toggle and the IPC listener — and nothing else. `pipeline`,
   `discovery`, `channel::{gate,reconnect}` and `trust` have no
   production caller at all; `channel::{negotiate,handshake,noise,tcp}`
   are reachable only through `pair_with_candidate`'s real-handshake
   branch, which requires `discovered_candidates` to be non-empty, which
   only `note_discovered_peer` fills, which nothing outside tests calls.
   `daemon/README.md` states this plainly, so it isn't a discovery — but
   two consequences are: `DaemonLinkState` is a frozen constant in
   production ([H3](#h3)), and the root `README.md`'s POC roadmap reads
   as further along than the daemon README's own account ([D1](#d1)).

3. **The capture layer cannot do what the product requires.** All three
   platform adapters capture passively — macOS `ListenOnly`, Windows
   `CallNextHookEx` pass-through, Linux no `EVIOCGRAB`. "Switch which
   computer is receiving your input" is not implementable on top of that:
   input would reach both machines. Compounding it, the streaming gate is
   inverted relative to the contract's definition of `active`
   ([H1](#h1), [H2](#h2)).

Everything else is ordinary: real but bounded bugs, idiom drift, and
documentation that has aged past the code.

---

## Findings

Severity is about impact on a shipped product, not on the current test
suite — which is green throughout.

### Critical

<a id="c1"></a>
#### C1 — Every boot after the first leaves the daemon permanently unswitchable

`daemon/src/storage/device_repo.rs:134`, `daemon/src/service/mod.rs:77`

`row_to_record` hardcodes `state: DeviceState::Disconnected` for every
row it loads, with a correct rationale: a stale row must never resurrect
a *peer* as `Active`. But nothing exempts this machine, and nothing ever
puts it back. `ServiceState::load_or_seed` seeds `d1 = Active` only when
the database is empty; on the second run it takes the `else` branch and
every device arrives `Disconnected`.

The knock-on effects are total, because `Disconnected` is not a
switchable state anywhere:

- `switch_active_device` rejects every target with `device_not_switchable`.
- `next_switchable_device` finds nothing, so `switch_active_device_local`
  — the switch key, the product's defining interaction — returns without
  doing anything, forever.
- `pipeline::is_local_device_active` is permanently `false`.

Confirmed by execution: after one restart against the same database, all
three seeded devices report `Disconnected` and `switch_active_device("d2")`
returns `Err(DeviceNotSwitchable)`.

No test caught this because every service test constructs
`Storage::open_in_memory()` fresh, which is always a first boot.
`removing_a_device_persists_across_a_reload` is the one test that
simulates a restart, and it only asserts absence.

**Fixed** — see [R1](#r1).

<a id="c2"></a>
#### C2 — `switch_active_device` can ack success having left no active device

`daemon/src/service/mod.rs:234`

The eligibility check runs under a read lock, the lock is dropped, then
`SWITCH_DEBOUNCE` (400 ms) is awaited, then the mutation runs under a
write lock. Anything can happen in that window. If the target is removed
during it, the mutation loop matches no device for the `Active` branch
but *does* hit the `else if` branch for the currently-active device — so
it demotes the active device, activates nothing, and returns `Ok(())`.

Confirmed by execution: with a `remove_device("d2")` landing 50 ms into
the debounce, `switch_active_device("d2")` returned `Ok(())`, `d1` was
left `Inactive`, and the active-device count was `0`. The UI receives an
ack and a `devices_changed` with no active device — a state the contract
has no name for.

**Fixed** — see [R2](#r2).

### High

<a id="h1"></a>
#### H1 — No platform adapter can suppress local input, so switching cannot work

`platform/src/macos/capture.rs:146`, `platform/src/windows/capture.rs:210,233`,
`platform/src/linux/capture.rs:87`

- macOS creates its tap with `CGEventTapOptions::ListenOnly`; the comment
  at line 157 acknowledges the return value is ignored by the OS.
- Windows' hook procedures end in `CallNextHookEx(...)`, passing every
  event on to the rest of the chain.
- Linux reads evdev nodes without `EVIOCGRAB`, so the kernel keeps
  delivering to normal consumers.

All three are correct for a *hotkey listener*, which is the only consumer
wired up today. None of them can implement the product: `README.md` says
"switching which computer is currently receiving your input", and
`data-model.md:39` defines `active` as "currently receiving
keyboard/mouse input". Once input actually streams, a keystroke typed
while a remote device is active would land on both machines.

This isn't a small patch — suppression is per-platform, and each platform
makes it a different kind of hard (macOS needs a `Default` tap plus the
Accessibility grant and must return `None` to swallow; Windows needs
`LRESULT(1)` and careful re-entrancy handling; Linux needs `EVIOCGRAB`
plus a way to release the grab if the daemon dies). It should be
scheduled as its own track before any more of the streaming path is
built on the assumption that it exists.

<a id="h2"></a>
#### H2 — The streaming gate is inverted relative to the contract

`daemon/src/pipeline/mod.rs:32,48`

`send_while_active` forwards captured events to the peer exactly when the
**local** device's state is `Active`. Per `data-model.md:39`, `active`
means "currently receiving keyboard/mouse input… this machine, 'This
device', is `active` by default when nothing else is." So the local
device being `Active` is precisely the case where input should *stay*
local; streaming should happen while a *remote* device is active.

As written, the default state (this machine active, nothing paired
in use) is the one that streams everything to the peer.

A second problem sits behind the same function: it holds one
`Box<dyn Channel>` and no device identity, so with three or more paired
devices it has no way to send to *which* device is active — it sends to
whichever peer it happens to hold. The gate needs to be
"is some remote device active, and is this channel that device's?", not
a boolean.

Latent today only because nothing calls `send_while_active` outside
tests. Not fixed here: correcting the polarity without the "which peer"
half would just move the bug.

<a id="h3"></a>
#### H3 — `DaemonLinkState` is a constant in production

`daemon/src/service/mod.rs:101,188`, `daemon/src/channel/reconnect.rs:41`

`DaemonService` initialises `link_state` to `Connected` and **never sends
on `link_state_tx` again** — verified by grep across the workspace: the
only writes to any `DaemonLinkState` sender are inside
`reconnect::maintain_connection`, which takes its *own* sender and is
never called from `main.rs`.

So a real UI over IPC sees `connected` at connect time and never anything
else, for the life of the daemon. Every downstream behaviour keyed on it
is dead:

- the tray's amber/gray/red dots and all three recovery banners,
- `history_logger`'s `link_state_changed` rows (`storage/history_logger.rs:71`)
  — the `connection_history` table will never contain one,
- `daemon-ipc.md`'s entire "Link health" state machine.

The fix is wiring, not new logic: pass `DaemonService`'s own
`link_state_tx` into `maintain_connection` rather than letting the caller
construct a second channel. Left alone here because it can't be done
meaningfully until there is a peer-connection loop in `main.rs` to hang
it off — but it's the smallest of the three unwired pieces and the one
with the most visible payoff.

<a id="h4"></a>
#### H4 — The device signing key is stored world-readable

`daemon/src/storage/identity_repo.rs`, `daemon/src/main.rs:75`

`ipc/auth.rs` is careful: it `chmod 0600`s the IPC token and has a test
asserting the mode. That token is a local capability — losing it lets a
local process talk to your daemon.

The ed25519 **private key** — the long-term identity every peer's trust
gate authenticates against — is stored unencrypted in `flow.db`, created
via `Connection::open(path)` with no mode set, so it lands at whatever
the umask gives (typically `0644`). Any local user can read it, forge
this device's identity to any paired peer, and pass the trust gate.

The far less sensitive secret is protected and the more sensitive one
isn't. Minimum fix: `chmod 0600` the database file at creation, alongside
the same `#[cfg(unix)]` block `auth.rs` already has. Proper fix: OS
keychain (Keychain / DPAPI / Secret Service), which
`docs/release/bundling-and-release.md` should carry as an open decision.

<a id="h5"></a>
#### H5 — Eight of nine UI command call sites drop the command's errors

`flutter/lib/features/**`

`DaemonRepository`'s commands are documented to *throw*
`DaemonCommandException` on rejection. Exactly one call site handles it
(`tray_popover.dart:43`, `_switchTo`). The other eight discard the
returned `Future` entirely:

| Call site | Command | Realistic rejection |
|---|---|---|
| `tray_popover.dart:107` | `startPairing` | `pairing_in_progress` |
| `onboarding_flow.dart:44` | `startPairing` | `pairing_in_progress` |
| `onboarding_flow.dart:122` | `requestPermission` | `permission_already_granted` |
| `onboarding_flow.dart:130` | `pairWithCandidate` | `pairing_not_ready`, `candidate_not_found` |
| `tray_pairing_view.dart:40` | `cancelPairing` | `pairing_not_active` |
| `tray_pairing_view.dart:193` | `pairWithCandidate` | `candidate_not_found` |
| `advanced_section.dart:71` | `requestPermission` | `permission_already_granted` |
| `input_section.dart:174` | `setSwitchKey` | `invalid_switch_key` |

Plus `advanced_section.dart:81`, which `await`s `resetSettings()` in an
async callback with no `try`, and `devices_section.dart:118` /
`dashboard_section.dart:212` for `removeDevice`.

In Flutter an unhandled `Future` error reaches the zone handler and shows
as a red-screen / console error, with nothing shown to the user about
what actually failed. These aren't hypothetical: double-tapping "Pair"
throws `pairing_in_progress` today against the mock.

The systemic fix is a small helper (`_run(command, onError: toast)`)
rather than eleven `try`/`catch` blocks; a lint (`unawaited_futures` /
`discarded_futures`) would keep it from regressing. Not applied here —
it touches eight files and deserves its own change.

<a id="h6"></a>
#### H6 — The dev harness crashes on its first tap in IPC mode

`flutter/lib/features/harness/dev_harness.dart:139`

`app.dart` sets `home: const DevHarness()`, so the harness *is* the app's
entry point. Its Connection control did
`(ref.read(daemonRepositoryProvider) as MockDaemonRepository)` — an
unchecked downcast. Under
`--dart-define=FLOW_DAEMON_MODE=ipc`, the mode that exists specifically
for daemon integration testing, the provider returns an
`IpcDaemonRepository` and the first tap throws `TypeError`.

It also breaks the ground rule stated in `daemon_repository.dart:11`:
"no widget or provider may depend on `MockDaemonRepository`… directly".

**Fixed** — see [R3](#r3).

### Medium

<a id="m1"></a>
#### M1 — One malformed frame kills every UI stream for the life of the process

`flutter/lib/data/ipc_daemon_repository.dart:76`

`_handleMessage` ran `jsonDecode(data as String) as Map<String, dynamic>`
unguarded inside a `StreamSubscription` callback. A binary frame, a
truncated payload, or a payload shape this build doesn't recognise threw
out of the callback — which doesn't drop the frame, it tears down the
subscription. Every `watch*` stream would go permanently silent while the
socket stayed open, and the UI would sit on stale state with no
indication anything was wrong.

The forward-compatibility angle makes this reachable without any
corruption: `daemon-ipc.md:58` promises a newer daemon can add values
without breaking clients, but `Enum.values.byName` throws on an unknown
name, so a newer daemon's `DeviceState` or `PairingStage` variant is
exactly the malformed-payload case.

The daemon's own server already does the right thing here
(`ipc/server.rs:214`: "drop it rather than crash the connection").

**Fixed** — see [R4](#r4).

<a id="m2"></a>
#### M2 — `identity.public_key` is persisted and is not the public key

`daemon/src/storage/identity_repo.rs:75`

`generate()` fills `public_key` and `private_key` with two *independent*
random draws. They have no cryptographic relationship. `DeviceIdentity`
(`identity/mod.rs:36`) sidesteps this correctly — it treats
`private_key` as a seed and derives the real public key — and its doc
comment explains why.

But the bogus column stays in the schema, `NOT NULL`, indistinguishable
from a real key. Any future code that reads `identity.public_key`
believing the column name gets 32 bytes of noise, and the failure mode is
"this peer's signature never verifies", which is a miserable thing to
debug.

Two clean options: drop the column in a follow-up migration and let
`DeviceIdentity` be the only source, or have `load_or_generate` derive
and store the real public key. Not fixed here — it needs a migration, and
the choice between the two is a design call.

<a id="m3"></a>
#### M3 — The daemon reports macOS permission copy on every OS

`daemon/src/service/mod.rs:105`

`load_or_seed` hardcoded `name: "Accessibility access"`. That is the
macOS wording. `flow_core::permission`'s own doc comment states the
design intent: `name` is daemon-supplied "rather than derived
client-side, so the UI never hardcodes per-OS permission copy" — which
only works if the daemon reports the right one. A Linux user would be
told to grant "Accessibility access".

**Fixed** — see [R5](#r5).

<a id="m4"></a>
#### M4 — `history_logger` identifies devices by display name

`daemon/src/storage/history_logger.rs:118`

`log_pairing_transition` resolves a newly paired device's id by matching
`PairingSession.target_name` against `Device.name`, falling back to
`String::default()` (an empty `device_id`) when it doesn't match.

The service layer explicitly rejected exactly this reasoning: the comment
at `service/mod.rs:834` calls out review gap #4 — three machines called
"MacBook Pro" are not one trust identity — and derives real `DeviceId`s
from proven public keys. The history logger then re-introduces the
name-as-identity assumption one layer down, and silently writes rows with
an empty `device_id` when it fails.

`on_pair_request_elapsed`/`on_real_pairing_request` know the real
`DeviceId`. Carrying it through the transition (or logging from the
service directly) removes the guess.

<a id="m5"></a>
#### M5 — No timeout on any IPC command

`flutter/lib/data/ipc_daemon_repository.dart:201`

`_sendCommand` registers a `Completer` and waits. The only thing that
ever completes it other than a reply is `_handleDone`. If the daemon
accepts the connection and then wedges — a deadlock, a blocked
`spawn_blocking` pool, a `switch_active_device` stuck on the state
lock — the `Future` hangs forever, and the UI hangs with it (the tray's
`_switchingDeviceId` spinner never clears).

A `.timeout(...)` completing with a synthetic `daemon_timeout`
`DaemonCommandException` would cost a few lines. Worth adding alongside
the [H5](#h5) error-handling pass, since both need the same call-site
change.

<a id="m6"></a>
#### M6 — The UI never notices the daemon going away

`flutter/lib/data/ipc_daemon_repository.dart:130,136`

`_handleTransportError` is an empty body with a comment. `_handleDone`
fails pending commands but emits nothing on `_linkState`. So when the
daemon dies, the last `link_state_changed` value it sent — `connected`,
by [H3](#h3) — stays on the stream, and the UI keeps rendering a green
dot against a dead socket.

`DaemonLinkState.disconnected` exists for precisely this, and its
documented UI treatment ("gray dot, banner with Retry") is already
implemented. Emitting `disconnected` from `_handleDone` is a one-line fix
that makes an already-built affordance work. There is also no reconnect:
once the socket closes the repository is inert for the process's
lifetime, and `_recover`'s Retry button ([M7](#m7)) does nothing.

Not fixed here only because it pairs naturally with [H3](#h3) and the
[H5](#h5) pass.

<a id="m7"></a>
#### M7 — Two tray affordances are decorative

`flutter/lib/features/tray/tray_popover.dart:55`

`_recover` handles `disconnected` and `error` by showing a
`'Reconnected'` toast and doing nothing else — it claims a recovery that
did not happen. `reconnecting`'s "Cancel" is an empty `break` with a
comment explaining the contract has no cancel command.

The "Cancel" case is honestly documented and fine as a placeholder. The
`'Reconnected'` toast is worse than a no-op button, because it actively
tells the user something false. Removing the toast (leaving the button
inert until [M6](#m6)'s reconnect exists) would be more truthful.

<a id="m8"></a>
#### M8 — Linux capture busy-polls at 200 Hz

`platform/src/linux/capture.rs:24,87`

`run_capture_loop` sets every device non-blocking and spins over all of
them, sleeping 5 ms whenever a full pass read nothing. That's 200
wakeups/second forever, on a daemon whose stated principles are
"invisible by default" and whose success criteria include "runs
continuously without excessive resource usage". It also adds up to 5 ms
of avoidable latency to the very interaction the product is built around.

macOS and Windows don't have this problem — both block on a real OS
primitive (`CFRunLoop::run_current`, `GetMessageW`). Linux should too:
`poll(2)`/`epoll` over the device fds, or one blocking-read thread per
device. `evdev::Device` exposes `as_raw_fd`, so this is a contained
change inside one function.

<a id="m9"></a>
#### M9 — The Noise handshake has no timeout

`daemon/src/channel/noise.rs:89`

`NoiseChannel::handshake` awaits `recv_frame` with no deadline. A peer
that connects and then sends nothing pins the accepting task and its
buffers indefinitely. Because the trust gate necessarily runs *after* the
handshake (`channel/gate.rs`'s doc comment explains why that ordering is
forced), this is reachable by any unauthenticated peer that can open a
socket — the cheapest possible way to accumulate stuck tasks.

`tokio::time::timeout` around the handshake, mapping elapsed to
`ChannelError::AuthenticationFailed`, is the whole fix. Worth doing when
the accept loop lands, since that's when it becomes reachable.

<a id="m10"></a>
#### M10 — No version negotiation on the daemon-to-daemon wire

`core/src/channel/mod.rs`, `core/src/protocol/mod.rs`

`ChannelMessage` and `InputEvent` are serialised with plain
`serde_json` and no version field. Two daemons at different versions
fail at `serde_json::from_slice`, surfacing as
`ChannelError::Serialization` and a dropped connection with no
diagnosable reason — a bad experience for a product where the two ends
update independently, at their users' whim.

The pairing handshake is the natural place to exchange a protocol
version and refuse mismatches loudly. Related: `core::protocol`'s enums
are the only serde types in `flow-core` without
`#[serde(rename_all = "snake_case")]`, so they serialise as PascalCase
(`{"Keyboard":{"KeyDown":…}}`) while every UI-facing type is snake_case;
and `core/tests/wire_format.rs` pins the UI-facing shapes but not these,
so the cross-daemon wire format is the *only* one with no shape test.
Both are worth settling in the same change — before anything ships and
the format has to be kept.

### Low / idiomatic

<a id="l1"></a>
#### L1 — Rust

- **`core::state::AppState` is dead code.** `core/src/state/mod.rs` is
  never constructed anywhere; `ServiceState` superseded it. It also can't
  express what the daemon needs (no `remove`, no list, and `set_active`
  accepts an id that may not exist, silently making `active_device()`
  return `None`). Delete it.
- **`channel::gate` is unreachable.** `accept_trusted` is called only by
  its own tests — it's the [H1](#h1)/[H3](#h3) wiring gap in miniature.
  Keep it, but a `#[allow(dead_code)]`-style comment or a tracking issue
  would stop it reading as live.
- **`ConnectionHistoryRepo::recent` has no production caller.** Nothing
  in the IPC contract exposes history, so the table is written and never
  read. It also has no retention policy — `connection_history` grows
  without bound for the life of an install.
- **`#[serde(rename_all = "snake_case")]` on structs is a no-op.**
  `Device`, `PairingRequest`, `FlowSettings`, `SettingsPatch`,
  `PermissionStatus`, `SwitchKeyBinding`, `PairingSession`,
  `PairingCandidate` all carry it, and all already have snake_case
  fields. Harmless, but it implies a rename that isn't happening. It's
  load-bearing only on the enums.
- **Silent lossy parsing in the storage layer.**
  `device_repo::host_os_from_str` maps any unrecognised string to
  `Macos`; `settings_repo::pointer_sensitivity_from_str` maps anything to
  `Normal`. A corrupted or future-schema row is silently reinterpreted
  rather than reported. Prefer an explicit error, or at minimum a
  `tracing::warn!`.
- **Repositories are reconstructed per call.**
  `DeviceRepo::new(self.storage.clone())` appears at four call sites in
  `service/mod.rs`, `SettingsRepo::new(...)` at two. They're cheap
  (`Storage` is an `Arc` clone), but holding them as fields on
  `DaemonService` — the way `TrustGate` already holds its `DeviceRepo` —
  would read better and stop the pattern spreading.
- **Non-constant-time token comparison.** `ipc/server.rs:76` compares the
  presented token with `==`. The timing channel is not realistically
  exploitable against a local WebSocket handshake, but a
  constant-time compare is one line and removes the question.
- **Subprotocol matching is exact-string.**
  `Sec-WebSocket-Protocol` is a comma-separated *list*; `ipc/server.rs`
  compares the raw header value. A conforming client offering
  `token, flow-ipc` is rejected. Splitting on `,` and trimming would
  match the spec.
- **Modifier tokens are case-sensitive, key tokens aren't.**
  `hotkey/mod.rs:92`'s `modifier_for_token` matches `"Ctrl"` exactly,
  while `canonicalize` upper-cases key names precisely because casing
  conventions differ between layers. A custom binding using `"ctrl"`
  falls through to the literal-key path and can never match.
- **`InputCapture::stop()` is never called.** `hotkey/runner.rs:63`
  keeps the handle alive as `_capture` and lets it drop, with a comment
  noting that dropping doesn't stop the underlying capture. On macOS and
  Windows that leaks an OS-level tap/hook across daemon shutdown.
- **Injected input can be re-captured on Linux.**
  `linux/discovery.rs:15` enumerates *every* evdev node with keys or
  relative axes — including `LinuxInputInjector`'s own "Flow Virtual
  Input" uinput device. Today this is safe only by construction ordering
  (capture starts before the injector exists). Any capture restart after
  the injector is created creates an input feedback loop. Filter the
  virtual device out by name.
- **Discovery announces are unauthenticated and size-capped at 512 bytes.**
  `discovery/tcp.rs:102` — any LAN host can advertise an arbitrary name,
  OS and port and appear as a pairing candidate, and an announce over 512
  bytes is silently truncated into a parse failure. The trust gate limits
  the damage post-pairing; a spoofed *candidate* is still a real
  pre-pairing phishing surface. Also `spawn_listener` (`:131`) breaks its
  loop on any recv error, so one transient failure ends discovery
  permanently.
- **`main.rs` panics on every startup failure.** `expect` on the database
  open, the data directory, the token write, and the listener bind. For a
  background daemon that a service manager will restart in a loop, a
  logged error and a non-zero exit is friendlier than a panic backtrace.
  Relatedly, `tracing::info!("IPC auth token: {}", …display())` prints a
  *path* under a label that reads like it's about to print the secret.
- **`next_switchable_device` is non-deterministic with multiple actives.**
  `service/mod.rs:139` finds the active device by iterating a `HashMap`.
  Only reachable if the one-active invariant is already broken (which
  [C2](#c2) could do), but iteration order shouldn't decide the outcome.

<a id="l2"></a>
#### L2 — Dart / Flutter

- **Value equality is defined on three domain types out of seven.**
  `Device`, `PairingCandidate` and `SwitchKeyBinding` implement
  `==`/`hashCode`; `PairingSession`, `FlowSettings`, `SettingsPatch` and
  `PermissionStatus` don't. Since all of these arrive through
  `StreamProvider`, the ones without equality rebuild every listening
  widget on every emit even when nothing changed. Riverpod dedupes by
  `==`; these types opt out of that by omission. Make it uniform — by
  hand, or with `package:equatable`/`freezed` across the whole layer.
- **`copyWith` can't clear a nullable field.**
  `PairingSession.copyWith` (`pairing.dart:52`) uses `?? this.x` for
  `targetName` and `error`, so neither can be reset to `null` — the
  standard Dart `copyWith` trap. `PairingSession.idle` is the only reason
  it hasn't bitten; the state machine returns to idle by constructing a
  fresh object. Use sentinel objects or `freezed` if this needs to work.
- **No accessibility or keyboard affordances anywhere.** Every
  interactive control in the app — `FlowToggle`, `FlowButton`,
  `FlowSegmentedControl`, every device row, the window controls — is a
  bare `GestureDetector`. There is not one `Semantics`, `Tooltip`,
  `Focus`, `FocusableActionDetector`, `InkWell` or `MouseRegion` in
  `lib/`. Consequences: nothing is reachable by keyboard (no Tab, no
  Enter), screen readers see unlabelled boxes, and the cursor never
  changes on hover — the last of which is immediately visible on a
  desktop app aiming to "feel native on every OS". Wrapping the four
  primitives in `Semantics` + `FocusableActionDetector` fixes most of it
  in one place.
- **The dev harness is the shipped entry point.** `app.dart:26` sets
  `home: const DevHarness()`. Deliberate and documented — but it's why
  [H6](#h6) was a crash in the real app rather than in a debug screen.
- **`themeModeProvider` doc and default disagree.**
  `ui_providers.dart:14` defaults to `ThemeMode.dark` under a comment
  discussing `ThemeMode.system`.
- **`ipcTokenPath()` hardcodes `/`.** `ipc_auth.dart:17` builds
  `'$home/.flow/ipc.token'`. Works on Windows because Win32 accepts
  forward slashes, but `path.join` states the intent.
- **`loadIpcToken()` does sync file I/O on the UI isolate**, from a
  `Provider` factory during the first frame.
- **The mock's state machine is a third implementation of the same
  logic.** `MockDaemonRepository` (~300 lines of Dart) mirrors
  `DaemonService` (~800 lines of Rust), with the timings duplicated as
  constants in both plus a table in `daemon/tests/service_parity.rs`.
  This is a deliberate, documented choice and the parity tests are real —
  but three copies of one state machine is a standing maintenance cost
  worth re-evaluating now that `IpcDaemonRepository` exists and can drive
  a real daemon in tests.

<a id="d1"></a>
### Documentation

- **The root README overstates the POC roadmap.** `README.md`'s
  roadmap marks "Connectivity ✅ Implemented in the real Rust daemon"
  and "Keyboard ✅ … a real streaming pipeline (track G8)". The
  qualifications are there, but the checkmarks come first, and the
  honest version is two levels down in `daemon/README.md`: "no incoming
  connection accept loop runs inside `main.rs` itself yet". `daemon/README.md`
  is a model of accurate status writing; the root README should match its
  register — ◑ rather than ✅ for items 1, 2 and 3.
- **`daemon-ipc.md` had a bullet contradicting its own auth section.**
  The "out of scope" list said the contract "assumes the IPC channel
  itself is only reachable by the local user (Unix socket permissions /
  named pipe ACLs)" — which line 7 had already replaced with a token
  scheme, precisely because loopback TCP gives no such guarantee. Two
  daemon-thrown error codes (`unknown_command`, `invalid_payload`) and
  the silent-drop rule for unparseable frames were also undocumented.
  **Fixed** — see [R6](#r6).
- **A stale comment in `core::protocol`.** `InputEvent::timestamp_ms`'s
  doc claimed "H4's replay guard reuses this existing field as its
  sequence check" — which the sequence-number rewrite reversed, and which
  `ChannelMessage::Input`'s own comment now explicitly argues against.
  **Fixed** — see [R7](#r7).
- `todos.json` (52 KB) and `daemon/todos.json` are committed build-log
  artifacts that the source comments reference heavily ("track G8",
  "gap #32"). That coupling is fine while the project is one person, but
  it makes the code's comments unreadable to anyone without those files
  open. Consider promoting the durable rationale into the code and
  archiving the rest.

---

## Refactoring applied

Scoped deliberately: confirmed bugs with a contract-backed correct
answer, plus duplication and stale documentation. Anything needing a
design decision, a schema migration, or a change across many files was
written up above and left alone.

Verification after every change: `cargo clippy --workspace --all-targets
-- -D warnings` clean, `cargo fmt --check` clean, `cargo test --workspace`
169→172 passing, `flutter analyze` clean, `flutter test` 59→64 passing.

<a id="r1"></a>
**R1 — Restore the local device to `Active` on reload** ([C1](#c1)).
Added `restore_local_device_active` to `ServiceState::load_or_seed`. It
only fires when *no* device is `Active`, so it can't fight a future
change that does persist state, and it leaves peers `Disconnected` — the
`DeviceRepo` rule stays intact for the case it was written for. The
behaviour it restores is `data-model.md`'s own: "this machine, 'This
device', is `active` by default when nothing else is." Regression test:
`a_second_boot_still_has_a_switchable_device`.

<a id="r2"></a>
**R2 — Close the `switch_active_device` TOCTOU, and de-duplicate the
switch** ([C2](#c2)). The target's presence is now re-checked under the
write lock after the debounce, returning `device_not_found` if it
vanished. The mutation itself — which was duplicated verbatim between
`switch_active_device` and `switch_active_device_local` — is now one
`activate` function, so "exactly one device is `Active`" is enforced in a
single place. Regression test:
`removing_the_target_mid_debounce_is_rejected_and_leaves_the_active_device_alone`.

<a id="r3"></a>
**R3 — Guard the dev harness downcast** ([H6](#h6)). `as
MockDaemonRepository` became an `is` check with a toast explaining that
link state is daemon-reported in IPC mode. No more `TypeError` in the
mode built for daemon integration testing.

<a id="r4"></a>
**R4 — Make `IpcDaemonRepository` survive a malformed frame**
([M1](#m1)). `_handleMessage` now rejects non-`String` frames, catches
`FormatException`, requires a JSON object, and isolates payload parsing
so an unknown enum variant from a newer daemon leaves that channel's
previous value in place instead of killing every stream. This is the
Dart-side mirror of what `ipc/server.rs` already does. Four regression
tests added under
`a malformed frame is dropped, not fatal to the connection`.

<a id="r5"></a>
**R5 — Report the right permission name per OS** ([M3](#m3)). Added
`local_permission_name()`, returning "Accessibility access" / "Input
access" / "Input device access" to match
`PlatformChrome.permissionName`'s per-OS fallbacks. Note the deliberate
tradeoff: this diverges from `MockDaemonRepository`'s hardcoded
"Accessibility access" when the daemon runs on Linux or Windows. That is
the intended asymmetry — the mock impersonates a Mac for UI development;
a real daemon should report its real platform, which is the entire
argument in `flow_core::permission`'s doc comment.

<a id="r6"></a>
**R6 — Correct `daemon-ipc.md`** ([D1](#d1)). Documented
`unknown_command` and `invalid_payload`; documented that unparseable
frames are dropped silently in both directions and why (no `id` to
correlate a reply to); rewrote the stale "out of scope" bullet so it
describes what is actually deferred (encryption, and moving off TCP)
rather than contradicting the auth section above it.

<a id="r7"></a>
**R7 — Fix the stale `timestamp_ms` comment** ([D1](#d1)). Now states
that the field is capture-time metadata and explicitly *not* what replay
protection keys on, pointing at `ChannelMessage::Input`'s `sequence`.

**R8 — De-duplicate `hex_encode`.** `ipc/auth.rs` and `service/mod.rs`
each carried a byte-identical four-line copy. Now one
`pub(crate) fn hex_encode` in `daemon/src/lib.rs`, with a test. Written
with `fold` + `write!` rather than `map(format!).collect()` to avoid one
`String` allocation per byte.

**R9 — Simplify `SwitchKeyBinding.==`.** The hand-rolled index loop
became `listEquals` from `package:flutter/foundation.dart` — same
semantics, five lines shorter, and the idiom a Dart reader expects.

## Suggested order of work

1. **[C1](#c1)** — done, but persisting `DeviceState` properly (rather
   than restoring one device) is the real answer and should follow.
2. **[H1](#h1)** — input suppression. Everything downstream assumes it;
   the longer the streaming path grows without it, the more of that path
   is built on a false premise.
3. **[H3](#h3) + the `main.rs` accept loop** — this unlocks
   [M6](#m6), the link-health state machine, and the `connection_history`
   link rows all at once.
4. **[H2](#h2)** — fix the gate polarity together with "which peer is
   this channel for", not before.
5. **[H4](#h4)** — one `chmod` now, keychain later.
6. **[H5](#h5) + [M5](#m5) + [M6](#m6)** — one pass over the UI command
   call sites: a shared runner with error toasts, a timeout, and a
   `disconnected` emission on socket close.
7. **[M10](#m10)** — settle the cross-daemon wire format (version field,
   casing, a shape test) before anything ships and it has to be kept.
8. **[L2](#l2)** — accessibility and keyboard support in the four
   primitives.
