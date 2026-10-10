import { Check, Plus } from 'lucide-react';
import { Banner, Button, KeyCombo, StatusDot, type DotTone } from '@flow/design-system';
import { useClient, useFlow } from '../../state/FlowProvider';
import { effectiveLink, isReachable, localDevice, localOs, myRole, situation } from '../../state/selectors';
import { useAction } from '../../state/useAction';
import { useUi } from '../../state/ui';
import { DeviceGlyph } from '../DeviceGlyph';
import { osName } from '../../copy';

const LINK: Record<string, [string, DotTone]> = {
  connected: ['Connected', 'green'],
  connecting: ['Connecting…', 'orange'],
  reconnecting: ['Reconnecting…', 'orange'],
  disconnected: ['Not connected', 'gray'],
  no_computers: ['No computers yet', 'gray'],
  error: ['Needs attention', 'red'],
  permission_required: ['Needs permission', 'red'],
};

const BANNER: Record<string, { tone: 'orange' | 'red' | 'gray'; title: string; detail: string }> = {
  reconnecting: { tone: 'orange', title: 'Reconnecting…', detail: 'Trying to reach your other computer.' },
  disconnected: { tone: 'gray', title: 'Not connected', detail: 'Your other computer is unavailable.' },
  error: { tone: 'red', title: 'Sharing is paused', detail: 'Check your other computer and try again.' },
  permission_required: {
    tone: 'red',
    title: 'Flow needs permission',
    detail: 'Allow access to share your keyboard.',
  },
};

/** Compact popover (360×480) for the tray: status, device switcher, quick actions. */
export function TrayView() {
  const s = useFlow();
  const client = useClient();
  const { run, busy } = useAction();
  const ui = useUi();
  const { navigate } = ui;
  // In the desktop app the popover is its own small window: anything bigger
  // than a switch opens the full window instead of a sheet in here.
  const inMain = (target: { route?: string; sheet?: 'pairing' | 'permission' }, local: () => void) =>
    client.hasDesktopShell ? void run(() => client.openMainWindow(target)) : local();
  const openPairing = () => inMain({ sheet: 'pairing' }, ui.openPairing);
  const openPermission = () => inMain({ sheet: 'permission' }, ui.openPermission);
  const [text, tone] = LINK[effectiveLink(s)] ?? LINK.connected!;
  const me = localDevice(s);
  const sit = situation(s);
  const controller = myRole(s) === 'controller';
  const activeId = s.controlState?.activeScreen === 'local' ? me?.id : s.controlState?.activeScreen;
  const roleConflictOnly =
    s.linkState === 'error' &&
    s.devices.some((d) => d.linkError) &&
    s.devices.filter((d) => d.linkError).every((d) => d.linkError === 'role_conflict');
  const banner = roleConflictOnly ? undefined : BANNER[effectiveLink(s)];

  const summary =
    sit.kind === 'controller-remote'
      ? `Controlling ${sit.target?.name ?? 'another computer'}`
      : sit.kind === 'controlled-driven'
        ? `${sit.controller?.name ?? 'Another computer'} is controlling this computer`
        : sit.kind === 'controller-home'
          ? `Using ${me?.name ?? 'this computer'}`
          : 'Ready to be controlled';

  return (
    <div className="fl-tray" data-testid="tray">
      <div className="fl-tray__head">
        <StatusDot tone={tone} pulse={tone === 'orange'} />
        <h1 className="t-headline" style={{ flex: 1 }}>
          Flow
        </h1>
        <span className="t-callout t-secondary">{text}</span>
      </div>
      {banner && (
        <div style={{ padding: '0 var(--s-3) var(--s-2)' }}>
          <Banner
            tone={banner.tone}
            title={banner.title}
            actions={
              s.linkState === 'permission_required' ? (
                <Button size="small" variant="prominent" onClick={openPermission}>
                  Allow…
                </Button>
              ) : s.linkState === 'error' || s.linkState === 'disconnected' ? (
                <Button size="small" onClick={() => void run(() => client.retryConnection())}>
                  Retry
                </Button>
              ) : undefined
            }
          >
            {banner.detail}
          </Banner>
        </div>
      )}
      <p
        className="t-callout t-secondary"
        style={{ padding: '0 var(--s-4) var(--s-2)' }}
        data-testid="tray-summary"
      >
        {summary}
      </p>
      <div className="fl-tray__list" role="group" aria-label="Computers">
        {s.devices.map((d) => {
          const active = d.id === activeId;
          const enabled = controller ? (d.isLocal ? true : isReachable(d)) : false;
          return (
            <button
              key={d.id}
              type="button"
              className="fl-tray__item"
              disabled={!enabled || busy}
              aria-current={active ? 'true' : undefined}
              data-testid={`tray-${d.id}`}
              onClick={() => void run(() => client.switchTo(d.isLocal ? 'local' : d.id))}
            >
              <DeviceGlyph os={d.os} size={18} />
              <span style={{ flex: 1, minWidth: 0 }}>
                <span style={{ display: 'block' }}>{d.isLocal ? `${d.name} (this computer)` : d.name}</span>
                <span className="t-footnote t-secondary" style={{ display: 'block' }}>
                  {osName(d.os)} ·{' '}
                  {d.linkError
                    ? 'Needs attention'
                    : d.state === 'disconnected'
                      ? 'Offline'
                      : active
                        ? 'Active'
                        : 'Connected'}
                </span>
              </span>
              {active && <Check size={16} strokeWidth={2} aria-label="Active" />}
            </button>
          );
        })}
        {sit.kind === 'controller-remote' && (
          <>
            <Button
              fullWidth
              style={{ marginTop: 'var(--s-3)' }}
              variant="prominent"
              disabled={busy}
              onClick={() => void run(() => client.switchTo('local'))}
            >
              Return to This Computer
            </Button>
            <p className="t-footnote t-secondary" style={{ textAlign: 'center', marginTop: 'var(--s-2)' }}>
              or press{' '}
              <KeyCombo
                keys={s.settings?.returnHomeKey?.keys ?? s.settings?.switchKey.keys ?? []}
                os={localOs(s)}
              />
            </p>
          </>
        )}
        {sit.kind === 'controlled-driven' && (
          <Button
            fullWidth
            style={{ margin: 'var(--s-2) 0' }}
            variant="prominent"
            disabled={busy}
            onClick={() => void run(() => client.releaseControl())}
          >
            Release Control
          </Button>
        )}
      </div>
      <div className="fl-tray__foot">
        <Button icon={<Plus size={14} strokeWidth={1.8} />} onClick={openPairing}>
          Add Computer…
        </Button>
        <span style={{ flex: 1 }} />
        <Button onClick={() => inMain({ route: '/home' }, () => navigate('/home'))}>Open Flow</Button>
      </div>
    </div>
  );
}
