/// Whether this build should treat itself as running in local development,
/// selected at build time via `--dart-define=FLOW_ENV=development` —
/// mirrors `repository_providers.dart`'s `FLOW_DAEMON_MODE` and
/// `ui_mode.dart`'s `FLOW_UI_MODE` conventions. Defaults to `false`
/// (production-like): unset, onboarding behaves exactly as it does for a
/// real user, which is also what `flutter test` gets since it never passes
/// this define either.
///
/// The one thing this currently unblocks is the onboarding permission
/// step's "Continue" gate (`features/onboarding/steps/permission_step.
/// dart`) — granting the OS Accessibility/input permission only works once
/// the app is a stable, installed bundle (macOS ties the grant to the
/// app's path in /Applications), which a `flutter run` build never is.
/// Blocking onboarding on a permission that structurally cannot be granted
/// yet would make the rest of the app unreachable during development.
const _rawFlowEnv = String.fromEnvironment(
  'FLOW_ENV',
  defaultValue: 'production',
);

const isDevelopmentEnv = _rawFlowEnv == 'development';
