# Remove Mock Runtime Data From Flow — implementation tracking

Branch: `fix/remove-mock-runtime-data` (off `main`, at `40efca0`)

## Goal

A fresh `flow-daemon` installation must have: real local device only, no
seeded fake remote devices, no seeded fake pairing candidates, and
`Disconnected` link state until a real peer connection completes.

## Plan / status — all done

- [x] Investigated every `seed_device_records` / `candidate_seeds` /
      `load_or_seed` call site and every test that depends on the current
      mock-parity fixture.
- [x] Added `hostname` crate + `current_host_os()`/`local_hostname()`
      helpers so the local device record reflects the real machine
      instead of a hardcoded "MacBook"/macOS.
- [x] Split `ServiceState::load_or_seed` into `from_storage` (production:
      seeds only the real local device, `link_state = Disconnected`,
      `candidates_pool = []`) and `seeded_for_test` (the exact mock-parity
      3-device/2-candidate/`Connected` fixture, kept `pub` — not
      `cfg(test)` — since `daemon/tests/*.rs` integration binaries link
      the crate normally and need to reach it too). Renamed
      `seed_device_records`/`candidate_seeds` to
      `mock_parity_device_records`/`mock_parity_candidates`.
- [x] `DaemonService::new` (production, `main.rs`'s only call site) now
      backed by `from_storage`. Added `DaemonService::new_seeded_for_test`
      backed by `seeded_for_test`. Mechanically renamed every existing
      test call site (31 in `service/mod.rs`'s own tests, plus
      `ipc/server.rs`, `ipc/dispatch.rs`, `logging.rs`,
      `storage/history_logger.rs`, and 3 `daemon/tests/*.rs` integration
      files) to the new test constructor — every one of them already
      depended on the mock-parity fixture (specific device ids "d2"/"d3",
      candidate ids, the local device's "MacBook" name, or `Connected`
      link state), so this is a pure rename with zero behavior change for
      existing tests.
- [x] Added 7 new regression tests exercising the real production path
      (`DaemonService::new`/`ServiceState::from_storage`) directly.
- [x] Audited every other `DaemonLinkState::Connected` write site
      (`main.rs`'s `run_peer_pipeline`, `channel/reconnect.rs`) — both
      already only fire on a real completed connection, not a seeded
      default; updated their doc comments, which had explicitly and
      correctly described the *old* bug as still-open.
- [x] Found and fixed two Flutter test files that also depend on the
      daemon spawning with the mock-parity fixture
      (`ipc_daemon_repository_manual_test.dart`,
      `daemon_ui_flow_e2e_test.dart`'s `startDaemon()`) — added an
      explicit `FLOW_DAEMON_SEED_MOCK_PARITY=1` opt-in env var honored
      only by `main.rs`, never set anywhere except these two test call
      sites.
- [x] Updated `daemon/README.md` (new "Removing mock runtime data"
      section, corrected 3 stale mentions of `load_or_seed`'s old
      `Connected` default) and `docs/testing/manual-testing-strategy.md`'s
      Tier 0 checklist.
- [x] Validation: `cargo fmt --all -- --check`,
      `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
      `cargo test --workspace`, `flutter test`,
      `flutter test --tags e2e --run-skipped test/e2e/daemon_ui_flow_e2e_test.dart`,
      `flutter test --tags manual --run-skipped test/data/ipc_daemon_repository_manual_test.dart`
      — all pass. Manual live-daemon check over the real WebSocket wire
      protocol (see report below) confirms the fix end to end, including
      restart persistence.

## Report

### 1. Files changed

- `daemon/Cargo.toml` — added `hostname = "0.4"`.
- `daemon/src/service/mod.rs` — the core fix: `ServiceState::from_storage`
  (real) vs `ServiceState::seeded_for_test` (mock fixture);
  `real_local_device_record`/`current_host_os`/`local_hostname` helpers;
  `mock_parity_device_records`/`mock_parity_candidates` (renamed from
  `seed_device_records`/`candidate_seeds`); `DaemonService::new` now real,
  `DaemonService::new_seeded_for_test` added; 7 new regression tests; ~30
  existing test call sites renamed to the new test constructor.
- `daemon/src/main.rs` — `daemon_service()` helper: real by default,
  `FLOW_DAEMON_SEED_MOCK_PARITY` env var opts into the mock fixture.
- `daemon/src/ipc/server.rs`, `daemon/src/ipc/dispatch.rs`,
  `daemon/src/logging.rs`, `daemon/src/storage/history_logger.rs` — test
  call sites renamed to `DaemonService::new_seeded_for_test`.
- `daemon/tests/ipc_protocol.rs`, `daemon/tests/pairing_over_channel.rs`,
  `daemon/tests/service_parity.rs` — same rename, with a comment on each
  explaining why that file still wants the mock fixture.
- `daemon/src/channel/reconnect.rs` — corrected stale doc comments that
  described the old bug as still open.
- `daemon/README.md` — new "Removing mock runtime data" section; fixed
  three other stale mentions of the old `Connected`-by-default behavior;
  updated the cross-language contract test instructions for the new env
  var.
- `docs/testing/manual-testing-strategy.md` — updated Tier 0 checklist to
  describe the real (not mock-seeded) fresh-daemon behavior.
- `flutter/test/data/ipc_daemon_repository_manual_test.dart`,
  `flutter/test/e2e/daemon_ui_flow_e2e_test.dart` — both spawn a real
  `flow-daemon` process and assert on the mock-parity fixture; added
  `FLOW_DAEMON_SEED_MOCK_PARITY=1` to how they launch it. **No
  `flutter/lib/` production code changed** — per the task's own
  instruction, the fix belongs entirely in the Rust daemon; the Flutter
  repository/provider architecture was untouched.
- `todos-remove-mock-runtime.md` — this file.

### 2. Mock runtime paths removed

- `ServiceState::load_or_seed` unconditionally seeding 3 hardcoded devices
  (`MacBook`/`Work Laptop`/`Desktop`) whenever the database was empty —
  removed from the production path entirely. A fresh install now gets
  exactly one device: this machine's own real hostname and real compiled
  OS, `Active`, not removable.
- `candidate_seeds()` (2 hardcoded pairing candidates, "Office Mac
  Mini"/"Studio Linux") being unconditionally loaded into
  `candidates_pool` at startup — production's `candidates_pool` now
  starts empty; `Pair New Device` only ever offers what
  `note_discovered_peer` (real discovery, track G7) adds.
- `link_state: DaemonLinkState::Connected` as a static production default
  — production now starts `Disconnected` and only transitions to
  `Connected` via `run_peer_pipeline`'s real post-handshake call to
  `DaemonService::set_link_state`, which was already correctly gated on a
  real completed connection (this part didn't need a code change, only a
  doc-comment correction — it was already right, but its own comments
  still described the seeded-`Connected` bug as unresolved).

All three mock behaviors are still available, but only ever behind an
explicit opt-in: `ServiceState::seeded_for_test`/
`DaemonService::new_seeded_for_test` for Rust tests, and
`FLOW_DAEMON_SEED_MOCK_PARITY=1` for the two Flutter tests that spawn a
real daemon process and need it to behave like the mock.

### 3. Tests added/updated

New (all in `daemon/src/service/mod.rs`, all passing):

- `fresh_real_state_has_only_the_local_device_no_fake_remotes`
- `fresh_real_state_starts_disconnected_with_no_seeded_candidates`
- `starting_pairing_on_a_real_daemon_finds_no_candidates_without_real_discovery`
- `a_real_discovered_peer_is_the_only_pairing_candidate_offered`
- `a_real_paired_device_survives_a_restart`
- `removing_a_real_device_persists_across_a_restart`
- `production_init_never_seeds_mock_parity_data` — the explicit guard
  requested: calls the real `DaemonService::new` and fails if it ever
  seeds more than the one real local device or claims `Connected` before
  a real peer link exists. This is what would catch someone re-wiring
  `from_storage` back to `seeded_for_test` inside `DaemonService::new`.

Updated (renamed constructor calls only, assertions unchanged — every one
of these already depended on the mock-parity fixture before this task):
~30 call sites across `daemon/src/service/mod.rs`'s test module,
`daemon/src/ipc/server.rs`, `daemon/src/ipc/dispatch.rs`,
`daemon/src/logging.rs`, `daemon/src/storage/history_logger.rs`,
`daemon/tests/ipc_protocol.rs`, `daemon/tests/pairing_over_channel.rs`,
`daemon/tests/service_parity.rs`.

Flutter (env var added to how the daemon process is launched, no
assertions changed): `flutter/test/data/ipc_daemon_repository_manual_test.dart`,
`flutter/test/e2e/daemon_ui_flow_e2e_test.dart`.

### 4. Commands/tests executed

```
cargo fmt --all -- --check                                          # clean
cargo clippy --workspace --all-targets --all-features -- -D warnings  # clean
cargo test --workspace                                               # 133+19+9+15+1+2+2+15 = all pass
cd flutter && flutter analyze                                        # no issues
cd flutter && flutter test                                           # 65 passed, 2 skipped (manual/e2e)
cd flutter && flutter test --tags manual --run-skipped test/data/ipc_daemon_repository_manual_test.dart
  # 14/14 pass (with FLOW_DAEMON_SEED_MOCK_PARITY=1)
cd flutter && flutter test --tags e2e --run-skipped test/e2e/daemon_ui_flow_e2e_test.dart
  # 8/8 pass
```

Manual live verification, real daemon process, real WebSocket wire
protocol (not just unit tests):

```
rm -rf ~/.flow ~/.local/share/flow-daemon
cargo run -p flow-daemon
# -> devices_changed: [{"id":"d1","name":"vm","os":"linux","state":"active"}]
#    (exactly one device — the real hostname of the machine that ran it)
# -> link_state_changed: "disconnected"
# -> pairing_session_changed: {"candidates":[],"stage":"idle",...}
```

Then restarted the same daemon against the same database and re-read the
first event: identical `last_seen` timestamp as the first boot, proving
it loaded the persisted record rather than reseeding.

### 5. Remaining limitations

- `ServiceState::seeded_for_test`/`DaemonService::new_seeded_for_test`
  and `mock_parity_device_records`/`mock_parity_candidates` are plain
  `pub` functions, not `#[cfg(test)]`-gated or behind a Cargo feature.
  This is deliberate — `daemon/tests/*.rs` integration binaries link the
  crate as a normal dependency and would not see a `cfg(test)`-gated item
  at all — but it means nothing at the type system level stops a future
  change to `main.rs` from calling `seeded_for_test` directly. The
  `production_init_never_seeds_mock_parity_data` regression test is the
  actual guard against that, not the compiler. A stricter alternative (a
  `test-support` Cargo feature gating these, enabled only via a
  self-referencing `[dev-dependencies]` entry) would close this gap at
  compile time, but wasn't implemented given the existing test already
  provides a fast, obvious failure signal.
- `FLOW_DAEMON_SEED_MOCK_PARITY` is a new, additive, opt-in-only
  environment variable read once in `main.rs`. It's undocumented outside
  `daemon/README.md`/this file and the two test files that set it — there
  is no validation preventing a real deployment from setting it by
  mistake beyond the startup `tracing::warn!` line.
- The local device's real name comes from `hostname::get()`, which can
  return an empty or non-UTF8 hostname on some misconfigured systems;
  this falls back to the literal string `"This device"` rather than
  failing daemon startup. Not testable in this sandbox in the "hostname
  command is broken" case, but the same "degrade gracefully" pattern the
  rest of this daemon already uses.
- This session's container has no `/dev/input`/`/dev/uinput` (no real
  keyboard/mouse hardware) and no second machine, so the "real discovery
  finds a real second daemon" path was verified via a hand-constructed
  `DiscoveredPeer` in a unit test
  (`a_real_discovered_peer_is_the_only_pairing_candidate_offered`), not
  against two genuinely separate physical machines. The actual
  cross-device UDP broadcast discovery mechanism itself was already
  fixed/verified in an earlier PR (`fix cross-device UDP broadcast
  discovery not reaching real LAN peers`) — this task only removes the
  fake candidates that used to hide alongside real ones.
