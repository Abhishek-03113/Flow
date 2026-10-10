/** UI-only preferences (never sent to the engine). Every access tolerates missing/blocked storage. */
const ONBOARDING = 'flow.onboarding_complete';

export function loadOnboardingComplete(): boolean {
  try {
    return localStorage.getItem(ONBOARDING) === '1';
  } catch {
    return false;
  }
}

export function saveOnboardingComplete(): void {
  try {
    localStorage.setItem(ONBOARDING, '1');
  } catch {
    /* ignore */
  }
}
