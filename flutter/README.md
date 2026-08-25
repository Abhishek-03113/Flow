# Flow UI

Flutter control-plane app for Flow (menu bar / system tray, pairing, settings, connection status). See [`docs/product/vision.md`](../docs/product/vision.md) for the architecture.

This directory currently holds only the Dart source layout. The native platform runners (`macos/`, `windows/`, `linux/`, etc.) aren't generated yet — this session had no Flutter SDK available to run `flutter create`. Once the SDK is available, generate them in place with:

```sh
flutter create --platforms=macos,windows,linux --org <your-org> .
```

then `flutter pub get` to fetch dependencies.

## Layout

- `lib/devices/` — paired devices and active-device display
- `lib/onboarding/` — first-run setup and initial pairing
- `lib/settings/` — connection, input, startup, and advanced settings
- `lib/tray/` — menu bar / system tray integration
- `lib/services/` — local IPC client to the Rust daemon
