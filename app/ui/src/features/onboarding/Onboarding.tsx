import { ChevronLeft } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { Role } from '../../client/types';
import { trayName } from '../../copy';
import { Button, KeyCombo } from '@flow/design-system';
import { useAction } from '../../state/useAction';
import { useClient, useFlow } from '../../state/FlowProvider';
import { loadOnboardingStep, saveOnboardingStep } from '../../state/prefs';
import { isReachable, localOs, remoteDevices } from '../../state/selectors';
import { PairingPanel } from '../devices/PairingPanel';
import { usePairingFlow } from '../devices/usePairingFlow';
import { PermissionGuide } from '../permissions/PermissionGuide';
import { WelcomeArt } from './Art';
import { RoleCards } from './RoleCards';

const STEPS = ['role', 'setup', 'pair'] as const;
type Step = (typeof STEPS)[number];

/** Where to start: the step saved before a restart (macOS asks for one after granting permissions). */
function initialStep(): number {
  const saved = STEPS.indexOf(loadOnboardingStep() as Step);
  return saved >= 0 ? saved : 0;
}

/**
 * Full-window first-run flow in three steps: choose what this computer does,
 * set up everything the OS must allow (one action), pair. A step with nothing
 * to do is skipped, and a restart resumes where the person left off.
 */
export function Onboarding({ onFinish }: { onFinish(): void }) {
  const s = useFlow();
  const client = useClient();
  const { run, busy } = useAction();
  const [step, setStep] = useState(initialStep);
  const [role, setRole] = useState<Role>(s.settings?.role ?? 'controller');
  const os = localOs(s);
  const flow = usePairingFlow();
  const primary = useRef<HTMLButtonElement>(null);
  const name = STEPS[step]!;
  const granted = s.permission?.granted ?? false;
  const restartRequired = s.permission?.restartRequired ?? false;
  const ready = granted && !restartRequired;

  useEffect(() => {
    primary.current?.focus();
    saveOnboardingStep(name);
  }, [name]);

  // Pairing runs only while the pairing page is visible.
  useEffect(() => {
    if (name === 'pair') void flow.start();
    else void flow.stop();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [name]);

  // Pairing works both ways: this computer may have accepted the other one's request.
  const linked = remoteDevices(s).find(isReachable);
  const paired = flow.display?.stage === 'paired' || linked !== undefined;

  // Once connected, stop this computer's own search.
  useEffect(() => {
    if (name === 'pair' && linked && flow.display?.stage !== 'paired') void flow.stop();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [name, linked?.id]);

  const goTo = (target: Step) => setStep(STEPS.indexOf(target));
  /** From the role step: straight to pairing when the OS needs nothing more. */
  const afterRole = () => goTo(ready ? 'pair' : 'setup');
  const finish = () => {
    saveOnboardingStep(null);
    onFinish();
  };
  const restart = async () => {
    // Come back to pairing after the relaunch: setup is done by then.
    saveOnboardingStep('pair');
    if (!(await run(() => client.restartApp()))) saveOnboardingStep(name);
  };

  const page = (() => {
    switch (name) {
      case 'role':
        return (
          <>
            <WelcomeArt />
            <h1 className="t-title">One keyboard and mouse for every computer</h1>
            <p className="t-body t-secondary" style={{ maxWidth: 440 }}>
              What should this computer do? You can change this later in General.
            </p>
            <div className="fl-onboard__body">
              <RoleCards value={role} onChange={setRole} />
            </div>
          </>
        );
      case 'setup':
        return (
          <>
            <h1 className="t-title">Let Flow use your keyboard, mouse and network</h1>
            <p className="t-body t-secondary" style={{ maxWidth: 440 }}>
              One-time setup so Flow can see and send keystrokes and mouse movement, and your computers can
              reach each other. Flow never records what you type.
            </p>
            <div className="fl-onboard__body">
              <PermissionGuide
                os={os}
                permission={s.permission}
                requesting={busy}
                onRequest={() => void run(() => client.requestPermission())}
                onOpenSettings={(n) => void run(() => client.openPermissionSettings(n))}
                onRecheck={() => void run(() => client.recheckPermissions())}
                onRestart={() => void restart()}
              />
            </div>
          </>
        );
      case 'pair':
        return (
          <>
            <h1 className="t-title">
              {role === 'controller' ? 'Add a computer to control' : 'Connect to a computer'}
            </h1>
            <p className="t-body t-secondary" style={{ maxWidth: 440 }}>
              Open Flow on the other computer too. Pairing happens once; after that your computers find each
              other automatically.
            </p>
            <div className="fl-onboard__body">
              <PairingPanel flow={flow} myRole={role} connectedTo={linked?.name} />
            </div>
            {paired && (
              <div
                className="fl-center"
                style={{ display: 'grid', gap: 'var(--s-2)' }}
                data-testid="onboarding-done"
              >
                <p className="t-body t-secondary">Press this shortcut to switch computers.</p>
                <KeyCombo keys={s.settings?.switchKey.keys ?? ['ScrollLock']} os={os} large />
                <p className="t-callout t-secondary" style={{ maxWidth: 400 }}>
                  Flow lives in your {trayName(os)}. Click it any time to switch computers or add another.
                </p>
              </div>
            )}
          </>
        );
    }
  })();

  const actions = (() => {
    switch (name) {
      case 'role':
        return (
          <Button
            ref={primary}
            variant="prominent"
            size="large"
            loading={busy}
            onClick={async () => {
              if (role === (s.settings?.role ?? 'controller') || (await run(() => client.setRole(role))))
                afterRole();
            }}
          >
            Get Started
          </Button>
        );
      case 'setup':
        return ready ? (
          <Button ref={primary} variant="prominent" size="large" onClick={() => goTo('pair')}>
            Continue
          </Button>
        ) : (
          <Button ref={primary} variant="plain" onClick={() => goTo('pair')}>
            Skip for Now
          </Button>
        );
      case 'pair':
        return paired ? (
          <Button ref={primary} variant="prominent" size="large" onClick={finish}>
            Start Using Flow
          </Button>
        ) : (
          <Button ref={primary} variant="plain" onClick={finish}>
            Skip for Now
          </Button>
        );
    }
  })();

  return (
    <div className="fl-onboard" data-testid="onboarding" data-step={name}>
      <div className="fl-onboard__top" data-tauri-drag-region>
        {step > 0 && (
          <Button
            variant="plain"
            icon={<ChevronLeft size={16} strokeWidth={1.8} />}
            onClick={() => setStep(step - 1)}
          >
            Back
          </Button>
        )}
      </div>
      <div className="fl-onboard__page" key={name}>
        {page}
      </div>
      <div className="fl-onboard__foot">
        <div className="fl-onboard__actions">{actions}</div>
        <ol className="fl-dots" aria-label={`Step ${step + 1} of ${STEPS.length}`}>
          {STEPS.map((n, i) => (
            <li key={n} aria-current={i === step ? 'step' : undefined} />
          ))}
        </ol>
      </div>
    </div>
  );
}
