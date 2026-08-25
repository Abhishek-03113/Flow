# Bundling and Release Strategy

**Status: draft, not yet implemented.** Written now, ahead of code, for the same reason `docs/architecture/channels.md` and `docs/testing/manual-testing-strategy.md` were: so the shape of the work is agreed before it's built rather than improvised at release time. `daemon/todos.json` track **J** (`packaging-qa`) is the closest existing track to this — J4 scaffolds service-unit files, J3 covers cross-compilation — but neither track owns "how do we actually ship this to a user." This document does.

## What this document is not

It is not a GitHub Actions workflow, not a `.github/release.yml`, not an actual GitHub Release. Nothing here is executable yet — this is the plan those things will eventually implement, kept in `docs/release/` alongside the rest of the project's pre-code design docs, the same way `docs/architecture/channels.md` predates any Channel code.

## Current state (grounding, checked before writing this)

- `Cargo.toml` (workspace) and `flutter/pubspec.yaml` both already say `0.1.0` — already in sync, by coincidence rather than policy so far. This document turns that coincidence into a policy (see "Versioning" below).
- No CI configuration exists in the repo yet (`.github/workflows/` doesn't exist).
- No `packaging/` directory exists yet — `daemon/todos.json` J4 (launchd/systemd/Windows-service scaffolding) hasn't landed.
- `daemon/todos.json` tracks A/P/B/C/D are done (real daemon, real IPC, real persistence); E (platform input), F (switch-hotkey), G (Channels/networking), H (security), I (reliability), J (packaging) are not.

A release strategy written against a daemon that can't yet capture input or talk to another machine would be fiction. So this document is a **ladder**, not a single "how to ship v1.0.0" recipe — each rung is tied to what `daemon/todos.json` actually needs to be true first, and to the manual verification gates `docs/testing/manual-testing-strategy.md` already defines.

## What ships, and how the two halves become one product

Flow is two artifacts that must be installed and versioned together but run as two independent processes (vision.md §8: "the daemon should continue working even if the UI is closed"):

- **`flow-daemon`** (Rust binary) — installed once, registered to start at login/boot as an OS-level service, runs continuously whether or not the UI is open.
- **The Flutter app** — installed alongside it, launched by the user (or from a tray icon once real tray docking exists), purely a client of the already-running daemon over local IPC (`docs/contracts/`).

**One installer per platform installs both and registers the daemon's auto-start.** Not two separate downloads — vision.md's "installation takes minutes" success criterion means a user runs one installer, and by the time it finishes, both the daemon is running and the app is launchable. This is also the point where `daemon/todos.json` J4's scaffolded service-unit files (`launchd` plist, `systemd` unit) stop being scaffolding and become something an installer actually places and activates — J4 explicitly does not do that activation step; this document is where that step gets specified.

## Per-platform bundling

### macOS

- `flutter build macos --release` produces `Flow.app`. `flow-daemon`'s binary is embedded inside it (`Contents/MacOS/flow-daemon` or `Contents/Resources/`), so the `.app` a user drags to Applications carries both.
- The daemon is registered as a **`launchd` LaunchAgent** (per-user, not system-wide — it only needs to run while the user is logged in, and per-user avoids requiring an admin-privileged install step), installed by the app on first launch or by the installer itself. The plist template is `daemon/todos.json` J4's deliverable; this is where it gets copied to `~/Library/LaunchAgents/` and loaded.
- **Code signing + notarization are required**, not optional — an unsigned/unnotarized `.app` requesting Accessibility/Input Monitoring permissions (needed for E4/E5's `CGEventTap`) will be blocked or heavily warned against by Gatekeeper, undermining vision.md's "invisible by default" principle before the app even opens. Needs a Developer ID Application certificate, `codesign` with the hardened runtime, and `notarytool` submission. Exact entitlements needed are TBD until E4/E5 land and are tested against real Gatekeeper/TCC behavior on real hardware (`docs/testing/manual-testing-strategy.md` Tier 5) — do not guess and hardcode an entitlements list before that.
- Distribution artifact: a signed, notarized `.dmg`.

### Windows

- `flutter build windows --release` produces an app folder (exe + DLLs + `data/`). `flow-daemon.exe` ships alongside it in the same install directory.
- The daemon is registered as a **Windows Service** (preferred over a Startup-folder shortcut, since a service can start before any user logs in and matches "daemon can start at boot" — vision.md §15) or, if a full service proves too heavy for v1, a Scheduled Task running at login as a fallback; decide which when I/J are actually implemented, not here.
- Packaged with an installer builder (WiX Toolset or Inno Setup — either is fine, pick one when this is actually implemented rather than over-specifying now) that installs both binaries, registers the service, and creates a Start Menu entry.
- **Code signing is required** — an unsigned installer/exe pair gets flagged hard by Windows SmartScreen, which for a product whose whole pitch is "invisible, trustworthy background utility" is close to disqualifying. Needs an Authenticode code-signing certificate (standard OV is acceptable to start; EV avoids the SmartScreen reputation-building period but costs more — a budget decision to make at implementation time, not a technical one to resolve here). Sign both `flow-daemon.exe` and the installer.
- Distribution artifact: a signed installer `.exe` (or `.msi`, depending on the chosen builder).

### Linux

- `flutter build linux --release` produces a bundle directory. `flow-daemon` ships alongside it.
- The daemon is registered as a **systemd user unit** (`daemon/todos.json` J4's `.service` file, installed to `~/.config/systemd/user/` and enabled via `systemctl --user enable --now`), consistent with not requiring root for a per-user background utility.
- **Packaging format: start with an AppImage.** It's the lowest-friction format for a desktop-first product with no package-manager relationship yet (no PPA, no Flathub listing) — a user downloads one file, marks it executable, runs it. `.deb`/`.rpm` are a reasonable v1.x follow-up once there's a release cadence worth maintaining two more package formats for, not a v1.0.0 requirement.
- **Known packaging complication, flagged now rather than discovered late:** `uinput`/`evdev` access (E1/E2) needs the user in the `input` group or a udev rule granting device access — an AppImage has no postinstall hook to set this up automatically the way a `.deb`'s `postinst` script could. v1's Linux release needs either a first-run helper in the app that detects missing permission and walks the user through the one-time `udev` rule (consistent with how macOS/Windows already need a first-run permission prompt per the onboarding flow), or an accompanying install script. Decide the exact mechanism when E1/E2 and the AppImage packaging are both actually being built — this document flags the gap so it isn't a surprise then.
- Distribution artifacts: the AppImage itself, a `SHA256SUMS` file, and a GPG signature over that checksum file (no OS-level code-signing gate exists on Linux the way Gatekeeper/SmartScreen do, but a checksum + signature is still the baseline for "don't ask users to run an unverified binary").

## Versioning

**One version number for the whole product**, not independently versioned Flutter/Rust halves — `Cargo.toml`'s `workspace.package.version` and `flutter/pubspec.yaml`'s `version` are bumped together, in the same commit, every release. They already happen to both read `0.1.0`; this document turns that into an enforced policy rather than a coincidence. (A CI check that fails if they diverge is a natural fit for whenever `daemon/todos.json`/a Flutter equivalent covers CI — not built yet, noted here as the eventual enforcement mechanism.)

**The contract version (`docs/contracts/CHANGELOG.md`, currently `0.1.1`) and the Channels protocol version (`docs/architecture/channels.md`, currently `0.1.0`) are independent of the product version number.** A product release's notes should state which contract/protocol versions it implements (useful for debugging a mismatched UI/daemon pair, or eventually for compatibility checks between two paired machines running different product versions), but a contract patch doesn't force a product version bump and vice versa.

**SemVer**, standard meaning for this project: a `MAJOR` bump is a breaking change to the local IPC contract or the Channels wire protocol that isn't backward compatible (i.e., an old UI can't talk to a new daemon or vice versa); `MINOR` is new user-facing capability; `PATCH` is a bug fix with no contract change. Pre-1.0, breaking changes don't require a major bump (standard SemVer pre-1.0 exception) but still get a `CHANGELOG.md` entry.

**Git tagging:** annotated tags `vX.Y.Z` on `main` only, created after the version-bump commit merges. A root `CHANGELOG.md` (doesn't exist yet — recommended as the next concrete step whenever the first tagged release is actually being cut, in Keep a Changelog style) tracks product-level release notes; this is distinct from `docs/contracts/CHANGELOG.md`, which only tracks the wire contract's own history.

## The release ladder (where "v1" actually sits)

Vision.md §20 (POC Strategy) is explicit that production installation, auto-start, and "perfect native system-tray integration" are *not* first-version scope — they're deferred deliberately, not forgotten. This ladder makes that deferral concrete by tying each stage to real `daemon/todos.json` progress instead of a calendar date:

| Version | Gate (what must be true first) | What a user gets |
|---|---|---|
| `0.1.0` (current) | Tracks A/P/B/C/D done | Nothing shippable — run from source, `docs/testing/manual-testing-strategy.md` Tier 0 only |
| `0.2.0-alpha` | Track E (Linux) + F done | Real local input capture/injection + working switch-hotkey on Linux, still run-from-source, still single-machine (Tier 1-2 of the testing doc) |
| `0.3.0-alpha` | Track G done (TCP Channel at least) | Real cross-device input sharing between two Linux machines/VMs over Wi-Fi (Tier 3-4). Bluetooth and macOS/Windows still absent or unverified. |
| `0.4.0-beta` | Track H + I done | Encrypted, trust-gated, reconnecting — the product's core loop is trustworthy. **First packaged build**: Linux AppImage only, unsigned/uncertified, published as a GitHub Release pre-release with a clear "early access, Linux only" label. macOS/Windows builds may exist (cross-compiled) but are explicitly marked unverified, not offered for general download. |
| `0.9.0-rc` | macOS/Windows manually verified on real hardware (`docs/testing/manual-testing-strategy.md` Tier 5), all three platforms code-signed/notarized, daemon auto-start registration actually implemented (not just J4's scaffolding) | Installers for all three platforms, feature-complete, in a release-candidate testing window |
| `1.0.0` | Full Tier 6 manual acceptance checklist passed on real hardware, at least one full release-candidate cycle with no regressions found | General availability: signed/notarized installers for macOS/Windows/Linux, daemon auto-starts at login on all three, tray integration real, published via GitHub Releases |

Skipping a rung (e.g., shipping a macOS build before Tier 5 has actually happened on real Apple hardware) is the specific mistake this ladder exists to prevent — cross-compile-checked (`cargo check --target ...`, per `daemon/todos.json` E4-E7) is not the same claim as "verified," and a release should never imply otherwise.

## Distribution channel

**GitHub Releases only for v1.0.0.** One release per version, with per-platform artifacts (`.dmg`, installer `.exe`/`.msi`, `.AppImage` + `SHA256SUMS` + `.sig`) attached to the same tagged release, and release notes stating the contract/protocol versions implemented (see "Versioning" above).

**Explicitly out of scope for v1.0.0** (matches the project's habit, established in `daemon/todos.json` and `docs/architecture/channels.md`, of stating exclusions rather than leaving them implicit):

- Package-manager distribution (Homebrew cask, `winget`, Flathub, a `.deb`/`.rpm` repo) — plausible v1.x follow-ups, not required to call v1.0.0 done.
- App Store / Microsoft Store distribution — different review/entitlement constraints (notably, the Mac App Store sandbox is hard to reconcile with the Accessibality/CGEventTap access E4 needs) that are a separate decision, not a v1 blocker to resolve now.
- An in-app auto-update mechanism. v1.0.0 users update by downloading a new installer from GitHub Releases; a "check for update" prompt is reasonable v1.x scope, not v1.0.0.
- Crash reporting / telemetry of any kind — no decision has been made about whether Flow collects anything, and shipping telemetry silently would contradict vision.md's "invisible by default" framed as a trust property, not just a UX one. If this is ever added, it needs its own explicit, documented, opt-in decision — not something this release doc backs into.
- CI/CD automation itself (a GitHub Actions build-and-release pipeline). This document describes what such a pipeline would need to produce; building the pipeline is future work, tracked wherever `daemon/todos.json`'s J track (or a new one) picks it up — not written here per the instruction that release docs live in `docs/release/`, not as actual workflow files.

## Open decisions to make at implementation time, not now

Flagged rather than resolved, so this document doesn't quietly pretend to have decided things it hasn't:

- WiX vs. Inno Setup for the Windows installer.
- Windows Service vs. Scheduled Task for the daemon's Windows auto-start.
- Exact macOS entitlements list (depends on E4/E5's real implementation and real Gatekeeper/TCC testing).
- The Linux `uinput`/udev first-run permission mechanism (in-app helper vs. install script).
- Whether a Developer ID / Authenticode / GPG signing identity is obtained personally or through an eventual organization — a budget and process question, not a technical one.
