import { Check, RotateCw, ShieldAlert } from 'lucide-react';
import type { FixStep, HostOs, PermissionItem, PermissionStatus } from '../../client/types';
import { Badge, Button, CodeBlock, Row, Section, useToast } from '@flow/design-system';

/** Guidance only the UI knows how to phrase; the permission items themselves come from the engine. */
const NOTE: Partial<Record<HostOs, string>> = {
  macos: 'macOS asks you to approve this in System Settings. Flow can’t do it for you.',
  windows:
    'If you use Flow with apps that run as administrator, start Flow as administrator too. Windows blocks input to those apps otherwise.',
  linux:
    'On Wayland, capturing input can be limited. If Flow still can’t see your keyboard, try an X11 session.',
};

function StepList({ label, steps }: { label: string; steps: FixStep[] }) {
  const toast = useToast();
  if (steps.length === 0) return null;
  return (
    <ol className="fl-steps" aria-label={label}>
      {steps.map((step, i) => (
        <li key={i}>
          <span className="t-callout">{step.text}</span>
          {step.command && <CodeBlock onCopy={() => toast.show('Copied')}>{step.command}</CodeBlock>}
        </li>
      ))}
    </ol>
  );
}

/** Every distinct step of the missing items, in order (Linux items share one fix). */
function sharedSteps(items: PermissionItem[]): FixStep[] {
  const seen = new Set<string>();
  const steps: FixStep[] = [];
  for (const item of items)
    for (const step of item.howToFix?.steps ?? []) {
      const key = `${step.text}\u0000${step.command ?? ''}`;
      if (!seen.has(key)) {
        seen.add(key);
        steps.push(step);
      }
    }
  return steps;
}

/**
 * Everything the OS must allow, on one screen, with one primary action:
 * - Linux: "Set Up Automatically" grants input access and opens the firewall
 *   behind a single password prompt (a terminal command when that isn't possible).
 * - macOS: "Allow Access" shows both OS prompts and opens System Settings;
 *   once granted, "Restart Flow" applies it (macOS needs a fresh process).
 * - Windows: nothing to grant.
 * Shared by onboarding and the "Fix permission" sheet.
 */
export function PermissionGuide({
  os,
  permission,
  onRequest,
  onOpenSettings,
  onRecheck,
  onRestart,
  requesting,
}: {
  os: HostOs;
  permission: PermissionStatus | null;
  onRequest(): void;
  onOpenSettings(name: string): void;
  onRecheck(): void;
  onRestart(): void;
  requesting?: boolean;
}) {
  const items = permission?.items ?? [];
  const granted = permission?.granted ?? false;
  const restartRequired = permission?.restartRequired ?? false;
  const oneClick = permission?.oneClickSetup ?? false;
  const missing = items.filter((i) => !i.granted);
  const allowed = <Badge tone="green">Allowed</Badge>;
  const note = NOTE[os];

  const primary = (() => {
    if (restartRequired)
      return (
        <div
          className="fl-center"
          style={{ display: 'grid', gap: 'var(--s-2)' }}
          data-testid="permission-restart"
        >
          <p className="t-callout">
            Restart Flow to finish. macOS applies the new permission to a fresh start.
          </p>
          <div>
            <Button variant="prominent" icon={<RotateCw size={14} strokeWidth={1.8} />} onClick={onRestart}>
              Restart Flow
            </Button>
          </div>
        </div>
      );
    if (granted) return null;
    if (os === 'linux')
      return oneClick ? (
        <div className="fl-center" style={{ display: 'grid', gap: 'var(--s-2)' }}>
          <div>
            <Button variant="prominent" loading={requesting} onClick={onRequest}>
              Set Up Automatically
            </Button>
          </div>
          <p className="t-footnote t-secondary">You’ll enter your password once. No log out needed.</p>
        </div>
      ) : null;
    if (os === 'macos')
      return (
        <div className="fl-center">
          <Button
            variant="prominent"
            loading={requesting}
            onClick={() => {
              onRequest();
              if (missing[0]) onOpenSettings(missing[0].name);
            }}
          >
            Allow Access
          </Button>
        </div>
      );
    return (
      <div className="fl-center">
        <Button variant="prominent" loading={requesting} onClick={onRequest}>
          Allow…
        </Button>
      </div>
    );
  })();

  return (
    <div data-testid={`permission-guide-${os}`} style={{ display: 'grid', gap: 'var(--s-3)' }}>
      <Section>
        {items.length === 0 ? (
          <Row
            icon={<Check size={16} strokeWidth={1.6} />}
            iconTone={granted ? 'green' : 'orange'}
            title={granted ? 'No extra permission needed' : 'Permission needed'}
            subtitle={
              granted
                ? 'Your computer lets Flow share your keyboard and mouse without a prompt.'
                : 'Allow Flow to share your keyboard and mouse.'
            }
            trailing={granted ? allowed : null}
          />
        ) : (
          items.map((item) => (
            <div key={item.name} data-testid="permission-item" data-granted={item.granted}>
              <Row
                icon={
                  item.granted ? (
                    <Check size={16} strokeWidth={1.6} />
                  ) : (
                    <ShieldAlert size={16} strokeWidth={1.6} />
                  )
                }
                iconTone={item.granted ? 'green' : 'orange'}
                title={item.name}
                subtitle={
                  item.granted ? 'Flow has access.' : (item.howToFix?.summary ?? 'Flow needs access.')
                }
                trailing={
                  item.granted ? (
                    allowed
                  ) : os === 'macos' ? (
                    <Button size="small" onClick={() => onOpenSettings(item.name)}>
                      Open Settings
                    </Button>
                  ) : (
                    <Badge tone="orange">Needed</Badge>
                  )
                }
              />
              {!item.granted && os === 'macos' && (
                <StepList label={`How to allow ${item.name}`} steps={item.howToFix?.steps ?? []} />
              )}
            </div>
          ))
        )}
      </Section>
      {primary}
      {!granted && os !== 'macos' && missing.length > 0 && (
        <StepList
          label="How to allow Flow"
          // The button above is that step; the rest (the terminal command, ports, log out) still helps.
          steps={sharedSteps(missing).filter((s) => !/Set Up Automatically/.test(s.text))}
        />
      )}
      {!granted && (
        <div style={{ display: 'flex', justifyContent: 'center' }}>
          <Button size="small" variant="plain" onClick={onRecheck}>
            Check Again
          </Button>
        </div>
      )}
      {note && (
        <p className="t-footnote t-secondary" style={{ textAlign: 'center' }}>
          {note}
        </p>
      )}
    </div>
  );
}
