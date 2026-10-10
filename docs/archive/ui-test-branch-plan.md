# `test/UI-test` — end-to-end two-daemon bring-up

**Goal:** get two `flow-daemon` instances on one machine to discover, pair,
and stream keyboard/mouse events **both directions**, headlessly, with
enough logging to see every hop — so "nothing flows" becomes a diagnosable
signal instead of a black box.

**Non-goal:** production behaviour changes. Every new env var below is
inert when unset; `cargo test --workspace` stays green throughout.

## New environment variables (all opt-in, dev-only)

| Var | Effect |
|-----|--------|
| `FLOW_TRACE=1` | Verbose `TRACE`-level structured logging on the `flow::hop` target + thread ids / targets / line numbers in the fmt layer. Read once at startup; overrides the `settings.debug_logging` level. |
| `FLOW_SECURITY=secure\|insecure` | Selects the security seam impls. Default / unset = `secure` = today's behaviour exactly. `insecure` requires `FLOW_DEV=1` as well and prints a loud startup banner. |
| `FLOW_DEV=1` | Guard that must accompany `FLOW_SECURITY=insecure`. |
| `FLOW_TEST_HOOKS=1` | Registers the `debug_inject_input` IPC command (unknown command otherwise). |

## Components

1. **Verbose logging** — `daemon/src/logging.rs` + `daemon/src/devmode.rs`
   (new: env parsing). Hop instrumentation added at each lifecycle point,
   every call site marked `// [FLOW-HOP]` for greppability. Flutter:
   `lib/core/flow_log.dart` global logger gated by
   `--dart-define=FLOW_TRACE=1`, wired into the IPC repository boundary.

2. **Pluggable security** — `daemon/src/security/` (new module). Three
   trait seams, each with a `Secure` impl (today's code, moved not
   rewritten) and a `DevInsecure` impl:
   - `PeerAuth::{initiate,accept}` — `NoiseChannel` vs. a plaintext
     public-key exchange returning the raw channel.
   - `TrustPolicy::{is_trusted,has_any_trusted}` — `TrustGate` vs.
     `AllowAll`.
   - `PairingConsent::decide` — prompt-and-wait vs. `AutoAccept`.
   Call sites rewired: `request_real_pairing`, `accept_pairing_over`,
   `accept_incoming_peer_channel`, `dial_if_trusted`,
   `channel::gate::accept_trusted`, `main.rs`.

3. **`debug_inject_input`** — `FLOW_TEST_HOOKS`-gated IPC command feeding a
   synthetic `InputEvent` into the same capture stream `run_peer_pipeline`
   reads, so it travels the identical gate → sequence → channel → peer →
   inject path. Receiver-side confirmation is via the hop log
   (`role=receiver … injected seq=N`), not a new IPC event — the IPC
   contract's 6-event initial burst stays frozen.

4. **`daemon/examples/drive_two_daemons.rs`** — connects IPC to A + B,
   drives pair A↔B, switch, injects a hardcoded key + mouse vector each
   direction, asserts on the hop logs, prints a PASS/FAIL matrix.

## Owner vs receiver

Every hop logs `role`: `owner` = the machine whose capture produced the
event / that holds the active-peer send gate open. `receiver` = the
machine injecting. On one desktop both daemons inject into the same
session, so the hop log is the only way to attribute an event.
