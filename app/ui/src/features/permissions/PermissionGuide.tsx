import { Check, ShieldAlert } from 'lucide-react';
import type { HostOs, PermissionItem, PermissionStatus } from '../../client/types';
import { Badge, Button, CodeBlock, Row, Section, useToast } from '@flow/design-system';

/** Guidance only the UI knows how to phrase; the permission items themselves come from the engine. */
const NOTE: Partial<Record<HostOs, string>> = {
  macos: 'macOS asks you to approve this in System Settings. Flow can’t do it for you.',
  windows:
    'If you use Flow with apps that run as administrator, start Flow as administrator too. Windows blocks input to those apps otherwise.',
  linux:
    'On Wayland, capturing input can be limited. If Flow still can’t see your keyboard, try an X11 session.',
};

function Steps({ item }: { item: PermissionItem }) {
  const toast = useToast();
  const steps = item.howToFix?.steps ?? [];
  if (steps.length === 0) return null;
  return (
    <ol className="fl-steps" aria-label={`How to allow ${item.name}`}>
      {steps.map((step, i) => (
        <li key={i}>
          <span className="t-callout">{step.text}</span>
          {step.command && <CodeBlock onCopy={() => toast.show('Copied')}>{step.command}</CodeBlock>}
        </li>
      ))}
    </ol>
  );
}

/**
 * Per-OS permission guidance, shared by onboarding and the "Fix permission"
 * sheet. One row per permission the engine reports, each with its own state,
 * a one-sentence summary and ordered steps (commands are copyable), a button
 * to the OS Settings page where there is one, and "Check Again".
 */
export function PermissionGuide({
  os,
  permission,
  onRequest,
  onOpenSettings,
  onRecheck,
  requesting,
}: {
  os: HostOs;
  permission: PermissionStatus | null;
  onRequest(): void;
  onOpenSettings(name: string): void;
  onRecheck(): void;
  requesting?: boolean;
}) {
  const items = permission?.items ?? [];
  const granted = permission?.granted ?? false;
  const allowed = <Badge tone="green">Allowed</Badge>;
  const note = NOTE[os];

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
            trailing={
              granted ? (
                allowed
              ) : (
                <Button size="small" loading={requesting} onClick={onRequest}>
                  Allow…
                </Button>
              )
            }
          />
        ) : (
          items.map((item) => (
            <div key={item.name}>
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
                  ) : os === 'linux' ? (
                    <Badge tone="orange">Needs your OK</Badge>
                  ) : (
                    <Button
                      size="small"
                      loading={requesting}
                      onClick={() => {
                        onRequest();
                        onOpenSettings(item.name);
                      }}
                    >
                      Open System Settings
                    </Button>
                  )
                }
              />
              {!item.granted && <Steps item={item} />}
            </div>
          ))
        )}
      </Section>
      {!granted && (
        <div style={{ display: 'flex', justifyContent: 'center' }}>
          <Button size="small" onClick={onRecheck}>
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
