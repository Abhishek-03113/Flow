# Bundling and release

Flow ships as one desktop app per OS. The engine is a library linked into the Tauri app, so there is nothing else to install and no background service to register. The headless `flow-daemon` binary is built separately for servers and tests and is not part of the installers.

## Bundles

`app/src-tauri/tauri.conf.json` (product `Flow`, identifier `dev.flow.app`):

| OS | Targets | Notes |
|---|---|---|
| macOS | `.app`, `.dmg` (universal) | `Info.plist` carries the Accessibility / Input Monitoring / local-network usage descriptions. Needs signing and notarization for distribution (secrets below). |
| Windows | `.msi`, NSIS `.exe` | Per-user NSIS install. No elevation needed; apps running as administrator can only be driven by an elevated Flow. |
| Linux | `.deb`, `.AppImage` | Depends on `libwebkit2gtk-4.1`, GTK 3 and `libayatana-appindicator3`. The user must be in the `input` group and have a uinput rule (the app explains this on first run). |

Build locally on the target OS (no cross-compiling): `npm --prefix app/ui ci && cd app && npm --prefix ui exec -- tauri build` (or `just build`; the Tauri CLI must run from `app/`).

## Versioning

One number for the whole product: `Cargo.toml` `workspace.package.version`, `app/src-tauri/tauri.conf.json` `version` and `app/ui/package.json` `version` are bumped together in one commit, then tagged `vX.Y.Z`.

## CI release

`.github/workflows/release.yml` runs on `v*` tags: a matrix (macOS universal, Windows, Linux) installs the Tauri Linux dependencies where needed, builds the UI and calls `tauri-apps/tauri-action` with `projectPath: app`, which reads the config above and attaches the installers to a **draft** GitHub release. Signing is optional and driven by repository secrets: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` (macOS), `bundle.windows.signCommand` (Windows), `TAURI_SIGNING_PRIVATE_KEY[_PASSWORD]` (updater, when enabled). Unsigned builds work.

## Not verified yet

Installers were not built in this repository's Linux development environment, so the macOS/Windows bundles, notarization, autostart on those OSes and the tray on each OS are untested on real machines.
