/// Which top-level UI this build launches into, selected at build time
/// via `--dart-define=FLOW_UI_MODE=harness` — mirrors
/// `repository_providers.dart`'s `FLOW_DAEMON_MODE` convention. Defaults
/// to [UiMode.app]: unset, `flutter run` launches the real onboarding/
/// dashboard flow docked to a real OS tray icon and window, not the dev
/// harness.
enum UiMode { app, harness }

const _rawUiMode = String.fromEnvironment('FLOW_UI_MODE', defaultValue: 'app');

final UiMode uiMode = _rawUiMode == 'harness' ? UiMode.harness : UiMode.app;
