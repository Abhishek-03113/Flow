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

const ONBOARDING_STEP = 'flow.onboarding_step';

/** Where onboarding was left, so a restart (macOS permissions) resumes there. */
export function loadOnboardingStep(): string | null {
  try {
    return localStorage.getItem(ONBOARDING_STEP);
  } catch {
    return null;
  }
}

export function saveOnboardingStep(step: string | null): void {
  try {
    if (step === null) localStorage.removeItem(ONBOARDING_STEP);
    else localStorage.setItem(ONBOARDING_STEP, step);
  } catch {
    /* ignore */
  }
}
