import 'package:shared_preferences/shared_preferences.dart';

/// Whether first-launch onboarding has ever been completed — a UI-only
/// concern (`vision.md`: "UI and system functionality are separate"),
/// kept entirely client-side rather than added to the daemon contract.
/// Deliberately not derived from daemon state (e.g. "is any device
/// paired?"): a user can complete onboarding without pairing anything,
/// and re-pairing later shouldn't resurface the welcome flow.
const _onboardingCompleteKey = 'flow.onboarding_complete';

Future<bool> loadOnboardingComplete() async {
  final prefs = await SharedPreferences.getInstance();
  return prefs.getBool(_onboardingCompleteKey) ?? false;
}

Future<void> saveOnboardingComplete() async {
  final prefs = await SharedPreferences.getInstance();
  await prefs.setBool(_onboardingCompleteKey, true);
}
