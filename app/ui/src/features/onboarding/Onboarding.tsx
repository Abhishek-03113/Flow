import { ChevronLeft } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { Role } from '../../client/types';
import { trayName } from '../../copy';
import { Button, KeyCombo, Row, Section, Toggle } from '@flow/design-system';
import { useAction } from '../../state/useAction';
import { useClient, useFlow } from '../../state/FlowProvider';
import { isReachable, localOs, remoteDevices } from '../../state/selectors';
import { PairingPanel } from '../devices/PairingPanel';
import { usePairingFlow } from '../devices/usePairingFlow';
import { PermissionGuide } from '../permissions/PermissionGuide';
import { WelcomeArt } from './Art';
import { RoleCards } from './RoleCards';

const STEPS = ['welcome', 'role', 'permissions', 'pair', 'done'] as const;

/** Full-window first-run flow: Welcome → Role → Permissions → Pair → Done. */
export function Onboarding({ onFinish }: { onFinish(): void }) {
  const s = useFlow();
  const client = useClient();
  const { run, busy } = useAction();
  const [step, setStep] = useState(0);
  const [role, setRole] = useState<Role>(s.settings?.role ?? 'controller');
  const os = localOs(s);
  const flow = usePairingFlow();
  const primary = useRef<HTMLButtonElement>(null);
  const name = STEPS[step]!;

  useEffect(() => {
    primary.current?.focus();
  }, [step]);

  // Pairing runs only while the pairing page is visible.
  useEffect(() => {
    if (name === 'pair') void flow.start();
    else void flow.stop();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [name]);

  const next = () => setStep((i) => Math.min(i + 1, STEPS.length - 1));
  const granted = s.permission?.granted ?? false;
  // Pairing works both ways: this computer may have accepted the other one's request.
  const linked = remoteDevices(s).find(isReachable);
  const paired = flow.display?.stage === 'paired' || linked !== undefined;

  // Once connected, stop this computer's own search.
  useEffect(() => {
    if (name === 'pair' && linked && flow.display?.stage !== 'paired') void flow.stop();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [name, linked?.id]);

  const page = (() => {
    switch (name) {
      case 'welcome':
        return (
          <>
            <WelcomeArt />
            <h1 className="t-large-title">
              One keyboard and mouse.
              <br />
              Every computer.
            </h1>
            <p className="t-body t-secondary" style={{ maxWidth: 400 }}>
              Keep using the keyboard and mouse you already have. Move to your other computer with a shortcut
              or by crossing the screen edge.
            </p>
          </>
        );
      case 'role':
        return (
          <>
            <h1 className="t-title">What should this computer do?</h1>
            <p className="t-body t-secondary">You can change this later in General.</p>
            <div className="fl-onboard__body">
              <RoleCards value={role} onChange={setRole} />
            </div>
          </>
        );
      case 'permissions':
        return (
          <>
            <h1 className="t-title">Allow Flow to share your input</h1>
            <p className="t-body t-secondary" style={{ maxWidth: 440 }}>
              Your computer needs to let Flow see and send keystrokes and mouse movement. Flow never records
              what you type.
            </p>
            <div className="fl-onboard__body">
              <PermissionGuide
                os={os}
                permission={s.permission}
                requesting={busy}
                onRequest={() => void run(() => client.requestPermission())}
                onOpenSettings={(n) => void run(() => client.openPermissionSettings(n))}
                onRecheck={() => void run(() => client.recheckPermissions())}
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
              Pairing happens once. After that your computers find each other automatically.
            </p>
            <div className="fl-onboard__body">
              <PairingPanel flow={flow} myRole={role} connectedTo={linked?.name} />
            </div>
          </>
        );
      case 'done':
        return (
          <>
            <h1 className="t-large-title">You’re all set</h1>
            <p className="t-body t-secondary">Press this shortcut to switch computers.</p>
            <KeyCombo keys={s.settings?.switchKey.keys ?? ['ScrollLock']} os={os} large />
            <p className="t-callout t-secondary" style={{ maxWidth: 400 }}>
              Flow lives in your {trayName(os)}. Click it any time to switch computers or add another.
            </p>
            <div style={{ width: 'min(420px, 100%)' }}>
              <Section>
                <Row
                  title="Open Flow at login"
                  subtitle="Starts quietly in the background. You can change this in General."
                  trailing={
                    <Toggle
                      label="Open Flow at login"
                      checked={s.settings?.launchAtLogin ?? false}
                      onChange={(v) => void run(() => client.updateSettings({ launchAtLogin: v }))}
                    />
                  }
                />
              </Section>
            </div>
          </>
        );
    }
  })();

  const actions = (() => {
    switch (name) {
      case 'welcome':
        return (
          <Button ref={primary} variant="prominent" size="large" onClick={next}>
            Get Started
          </Button>
        );
      case 'role':
        return (
          <Button
            ref={primary}
            variant="prominent"
            size="large"
            loading={busy}
            onClick={async () => {
              if (role === (s.settings?.role ?? 'controller') || (await run(() => client.setRole(role))))
                next();
            }}
          >
            Continue
          </Button>
        );
      case 'permissions':
        return granted ? (
          <Button ref={primary} variant="prominent" size="large" onClick={next}>
            Continue
          </Button>
        ) : (
          <>
            <Button ref={primary} variant="prominent" size="large" disabled>
              Continue
            </Button>
            <Button variant="plain" onClick={next}>
              Skip for Now
            </Button>
          </>
        );
      case 'pair':
        return paired ? (
          <Button ref={primary} variant="prominent" size="large" onClick={next}>
            Continue
          </Button>
        ) : (
          <Button ref={primary} variant="plain" onClick={next}>
            Skip for Now
          </Button>
        );
      case 'done':
        return (
          <Button ref={primary} variant="prominent" size="large" onClick={onFinish}>
            Start Using Flow
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
