import { Check, Copy, ShieldAlert } from 'lucide-react';
import type { HostOs, PermissionStatus } from '../../client/types';
import { Badge, Button, Row, Section, useToast } from '../../design';

export const SETTINGS_URLS = {
  macAccessibility: 'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility',
  macInputMonitoring: 'x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent',
} as const;

/** Best effort: deep-link into the OS privacy pane (the shell opens custom schemes). */
export function openSettings(url: string): void {
  window.open(url, '_blank', 'noopener');
}

const LINUX_GROUP = 'sudo usermod -aG input $USER';
const LINUX_UDEV = `echo 'KERNEL=="uinput", GROUP="input", MODE="0660"' | sudo tee /etc/udev/rules.d/99-flow.rules`;

function CopyBlock({ text }: { text: string }) {
  const toast = useToast();
  return (
    <div style={{ display: 'flex', gap: 'var(--s-2)', alignItems: 'flex-start' }}>
      <code className="fl-pre" style={{ flex: 1 }}>
        {text}
      </code>
      <Button
        size="small"
        aria-label={`Copy command: ${text}`}
        icon={<Copy size={13} strokeWidth={1.6} />}
        onClick={() => {
          void navigator.clipboard?.writeText(text);
          toast.show('Copied');
        }}
      />
    </div>
  );
}

/** Per-OS permission guidance, shared by onboarding and the "Fix permission" sheet. */
export function PermissionGuide({
  os,
  permission,
  onRequest,
  requesting,
}: {
  os: HostOs;
  permission: PermissionStatus | null;
  onRequest(): void;
  requesting?: boolean;
}) {
  const granted = permission?.granted ?? false;
  const status = granted ? <Badge tone="green">Allowed</Badge> : <Badge tone="orange">Needs your OK</Badge>;

  if (os === 'macos') {
    return (
      <div data-testid="permission-guide-macos">
        <Section footer="macOS asks you to approve this in System Settings. Flow can't do it for you. After switching it on, you may need to reopen Flow.">
          <Row
            icon={<ShieldAlert size={16} strokeWidth={1.6} />}
            iconTone={granted ? 'green' : 'orange'}
            title="Accessibility"
            subtitle="Lets Flow send keystrokes and clicks to other computers."
            trailing={
              granted ? (
                status
              ) : (
                <Button
                  size="small"
                  loading={requesting}
                  onClick={() => {
                    onRequest();
                    openSettings(SETTINGS_URLS.macAccessibility);
                  }}
                >
                  Open System Settings
                </Button>
              )
            }
          />
          <Row
            icon={<Check size={16} strokeWidth={1.6} />}
            iconTone="gray"
            title="Input Monitoring"
            subtitle="Lets Flow notice your shortcut while another computer is in use."
            trailing={
              <Button size="small" onClick={() => openSettings(SETTINGS_URLS.macInputMonitoring)}>
                Open System Settings
              </Button>
            }
          />
        </Section>
        <ol
          className="t-callout t-secondary"
          style={{ margin: 0, paddingLeft: 'var(--s-5)', display: 'grid', gap: 'var(--s-1)' }}
        >
          <li>Open Privacy &amp; Security in System Settings.</li>
          <li>Choose Accessibility, then switch on Flow.</li>
          <li>Do the same under Input Monitoring.</li>
        </ol>
      </div>
    );
  }

  if (os === 'windows') {
    return (
      <div data-testid="permission-guide-windows">
        <Section footer="If you use Flow with apps that run as administrator, start Flow as administrator too. Windows blocks input to those apps otherwise.">
          <Row
            icon={<Check size={16} strokeWidth={1.6} />}
            iconTone="green"
            title="No extra permission needed"
            subtitle="Windows lets Flow share your keyboard and mouse without a prompt."
            trailing={status}
          />
        </Section>
      </div>
    );
  }

  return (
    <div data-testid="permission-guide-linux" style={{ display: 'grid', gap: 'var(--s-3)' }}>
      <Section footer="Flow reads keyboard and mouse devices and creates a virtual one. Both need access that Linux gives to the “input” group.">
        <Row
          icon={<ShieldAlert size={16} strokeWidth={1.6} />}
          iconTone={granted ? 'green' : 'orange'}
          title="Input device access"
          subtitle={
            granted ? 'Flow can read and create input devices.' : 'Flow can’t open your input devices yet.'
          }
          trailing={status}
        />
      </Section>
      {!granted && (
        <>
          <p className="t-callout">1. Add yourself to the input group:</p>
          <CopyBlock text={LINUX_GROUP} />
          <p className="t-callout">2. Allow the virtual keyboard (uinput):</p>
          <CopyBlock text={LINUX_UDEV} />
          <p className="t-callout t-secondary">
            Then log out and back in. Wayland sessions may limit capturing input; if Flow still can’t see your
            keyboard, try an X11 session.
          </p>
        </>
      )}
    </div>
  );
}
