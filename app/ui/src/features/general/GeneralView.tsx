import { Monitor, Moon, ShieldAlert, ShieldCheck, Sun } from 'lucide-react';
import { useState } from 'react';
import type { PointerSensitivity, Role } from '../../client/types';
import { trayName } from '../../copy';
import {
  Alert,
  Badge,
  Button,
  Row,
  Section,
  SegmentedControl,
  Toggle,
  useTheme,
  type ThemeChoice,
} from '@flow/design-system';
import { useClient, useFlow } from '../../state/FlowProvider';
import { localOs, myRole, remoteDevices } from '../../state/selectors';
import { useAction } from '../../state/useAction';
import { useUi } from '../../state/ui';
import { RoleCards } from '../onboarding/RoleCards';

declare const __APP_VERSION__: string;

export function GeneralView() {
  const s = useFlow();
  const client = useClient();
  const { run } = useAction();
  const { theme, setTheme } = useTheme();
  const { openPermission } = useUi();
  const [pendingRole, setPendingRole] = useState<Role | null>(null);
  const settings = s.settings;
  if (!settings) return null;
  const os = localOs(s);
  const role = myRole(s);
  const set = (patch: Parameters<typeof client.updateSettings>[0], msg?: string) =>
    void run(() => client.updateSettings(patch), msg);
  const changeRole = (r: Role) => void run(() => client.setRole(r), 'Role updated');

  return (
    <div data-testid="general">
      <section className="fl-section" aria-labelledby="role-h">
        <h2 id="role-h" className="fl-section__title">
          What this computer does
        </h2>
        <RoleCards
          value={role}
          onChange={(r) => {
            if (r === role) return;
            if (remoteDevices(s).length > 0) setPendingRole(r);
            else changeRole(r);
          }}
        />
        <p className="fl-section__footer">
          Each connection needs one computer that shares its keyboard and mouse and one that is controlled.
          You can change this at any time.
        </p>
      </section>

      {role === 'controller' && (
        <Section
          title="Sharing"
          footer="Turn one off to keep it on this computer while you work on another one."
        >
          <Row
            title="Share keyboard"
            subtitle="Typing goes to the computer you moved to."
            trailing={
              <Toggle
                label="Share keyboard"
                checked={settings.shareKeyboard}
                onChange={(v) => set({ shareKeyboard: v })}
              />
            }
          />
          <Row
            title="Share mouse"
            subtitle="The pointer, clicks and scrolling go to the computer you moved to."
            trailing={
              <Toggle
                label="Share mouse"
                checked={settings.shareMouse}
                onChange={(v) => set({ shareMouse: v })}
              />
            }
          />
          <Row
            title="Pointer speed on other computers"
            trailing={
              <SegmentedControl<PointerSensitivity>
                label="Pointer speed on other computers"
                value={settings.pointerSensitivity}
                onChange={(v) => set({ pointerSensitivity: v })}
                options={[
                  { value: 'low', label: 'Slower' },
                  { value: 'normal', label: 'Normal' },
                  { value: 'high', label: 'Faster' },
                ]}
              />
            }
          />
        </Section>
      )}

      <Section title="Startup">
        <Row
          title="Open Flow at login"
          subtitle="Flow starts quietly in the background."
          trailing={
            <Toggle
              label="Open Flow at login"
              checked={settings.launchAtLogin}
              onChange={(v) => set({ launchAtLogin: v })}
            />
          }
        />
        <Row
          title={`Show in ${trayName(os)}`}
          subtitle="Quick access to switch computers."
          trailing={
            <Toggle
              label={`Show in ${trayName(os)}`}
              checked={settings.showTrayIcon}
              onChange={(v) => set({ showTrayIcon: v })}
            />
          }
        />
        <Row
          title="Reconnect automatically"
          subtitle="Recover on its own after a dropped connection."
          trailing={
            <Toggle
              label="Reconnect automatically"
              checked={settings.autoReconnect}
              onChange={(v) => set({ autoReconnect: v })}
            />
          }
        />
        <Row
          title="Connect to known computers"
          subtitle="Reconnect to paired computers when Flow opens."
          trailing={
            <Toggle
              label="Connect to known computers"
              checked={settings.autoConnectPairedDevices}
              onChange={(v) => set({ autoConnectPairedDevices: v })}
            />
          }
        />
      </Section>

      <Section title="Appearance">
        <Row
          title="Theme"
          trailing={
            <SegmentedControl<ThemeChoice>
              label="Theme"
              value={theme}
              onChange={setTheme}
              options={[
                { value: 'system', label: 'System', icon: <Monitor size={13} strokeWidth={1.7} /> },
                { value: 'light', label: 'Light', icon: <Sun size={13} strokeWidth={1.7} /> },
                { value: 'dark', label: 'Dark', icon: <Moon size={13} strokeWidth={1.7} /> },
              ]}
            />
          }
        />
      </Section>

      <Section title="Permission">
        <Row
          icon={
            s.permission?.granted ? (
              <ShieldCheck size={16} strokeWidth={1.6} />
            ) : (
              <ShieldAlert size={16} strokeWidth={1.6} />
            )
          }
          iconTone={s.permission?.granted ? 'green' : 'orange'}
          title={s.permission?.name ?? 'Input access'}
          subtitle={
            s.permission?.granted
              ? 'Flow can share your keyboard and mouse.'
              : 'Flow needs this to share your keyboard and mouse.'
          }
          trailing={
            s.permission?.granted ? (
              <Badge tone="green">Allowed</Badge>
            ) : (
              <Button size="small" variant="prominent" onClick={openPermission}>
                Fix…
              </Button>
            )
          }
        />
      </Section>

      <Section title="About">
        <Row
          title="Flow"
          subtitle="One keyboard and mouse for every computer."
          trailing={<span className="t-secondary">Version {__APP_VERSION__}</span>}
        />
      </Section>

      <Alert
        open={pendingRole !== null}
        title="Change this computer’s role?"
        message="Other computers set up the same way will show a warning until one of you changes."
        onClose={() => setPendingRole(null)}
        actions={[
          {
            label:
              pendingRole === 'controlled'
                ? 'Let Another Computer Control This One'
                : 'Share This Keyboard and Mouse',
            variant: 'prominent',
            onClick: () => {
              const r = pendingRole!;
              setPendingRole(null);
              changeRole(r);
            },
          },
          { label: 'Cancel', onClick: () => setPendingRole(null), autoFocus: true },
        ]}
      />
    </div>
  );
}
