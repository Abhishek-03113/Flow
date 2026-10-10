import { Gauge, MousePointer2, Undo2 } from 'lucide-react';
import type { ReactNode } from 'react';
import { friendlyError, formatMs, formatUptime } from '../../copy';
import { Badge, KeyCombo, Row, Section, StatusDot, useToast } from '@flow/design-system';
import { useClient, useFlow } from '../../state/FlowProvider';
import { effectiveLink, localDevice, localOs, myRole, remoteDevices } from '../../state/selectors';
import { ArrangementEditor } from './ArrangementEditor';
import { StatusCard } from './StatusCard';

const LINK_TEXT = {
  connected: ['Healthy', 'green'],
  connecting: ['Connecting', 'orange'],
  reconnecting: ['Reconnecting', 'orange'],
  disconnected: ['Offline', 'gray'],
  no_computers: ['No computers yet', 'gray'],
  error: ['Needs attention', 'red'],
  permission_required: ['Needs permission', 'red'],
  not_responding: ['Not responding', 'red'],
} as const;

function Stat({ value, label }: { value: ReactNode; label: string }) {
  return (
    <div>
      <div className="fl-stat__value">{value}</div>
      <div className="fl-stat__label">{label}</div>
    </div>
  );
}

export function HomeView() {
  const s = useFlow();
  const client = useClient();
  const toast = useToast();
  const os = localOs(s);
  const me = localDevice(s);
  const role = myRole(s);
  const controller = role === 'controller';
  const [linkText, linkTone] = LINK_TEXT[effectiveLink(s)];
  const activeScreen = s.controlState?.activeScreen ?? 'local';
  const activeId = controller
    ? activeScreen === 'local'
      ? (me?.id ?? null)
      : activeScreen
    : activeScreen === 'local'
      ? (me?.id ?? null)
      : activeScreen;
  // A controlled computer only knows its own screen; the arrangement lives on
  // the controller. Show the controller beside it so the picture isn't one
  // lonely tile (read-only; where they really sit is set on the controller).
  const controllerDevice = s.controlState?.controllerDeviceId
    ? s.devices.find((d) => d.id === s.controlState?.controllerDeviceId)
    : undefined;
  const layout = (() => {
    if (controller || !controllerDevice || !me) return s.screenLayout;
    if (s.screenLayout.screens.some((r) => r.deviceId === controllerDevice.id)) return s.screenLayout;
    const mine = s.screenLayout.screens.find((r) => r.deviceId === me.id) ?? {
      deviceId: me.id,
      x: 0,
      y: 0,
      width: 1920,
      height: 1080,
    };
    const theirs = controllerDevice.screen ?? { width: 1920, height: 1080 };
    return {
      screens: [
        { deviceId: controllerDevice.id, x: 0, y: 0, width: theirs.width, height: theirs.height },
        { ...mine, x: theirs.width, y: 0 },
      ],
    };
  })();
  const hasLayout = layout.screens.length > 1;
  const diag = s.diagnostics;
  const settings = s.settings;
  const controllerName =
    (s.controlState?.controllerDeviceId &&
      s.devices.find((d) => d.id === s.controlState?.controllerDeviceId)?.name) ||
    'the other computer';

  return (
    <div className="fl-grid" data-testid="home">
      <StatusCard />

      {(hasLayout || remoteDevices(s).length > 0) && (
        <section aria-labelledby="arrange-h">
          <h2 id="arrange-h" className="fl-section__title">
            Screen arrangement
          </h2>
          <div className="fl-card">
            <ArrangementEditor
              layout={layout}
              devices={s.devices}
              activeId={activeId}
              editable={controller}
              onCommit={async (next) => {
                try {
                  await client.setScreenLayout(next);
                  return true;
                } catch (e) {
                  toast.show(friendlyError(e), 'error');
                  return false;
                }
              }}
            />
            <p className="t-callout t-secondary" style={{ marginTop: 'var(--s-3)' }}>
              {controller
                ? 'Drag computers to match how they sit on your desk. Move the pointer off an edge to cross to the next one.'
                : `${controllerDevice?.name ?? 'The computer that shares its keyboard and mouse'} sets up where the screens sit; open Flow there to change it.`}
            </p>
          </div>
        </section>
      )}

      <section aria-labelledby="health-h">
        <h2 id="health-h" className="fl-section__title">
          Connection
        </h2>
        <div className="fl-card fl-stats">
          <Stat
            label="Status"
            value={
              <span style={{ display: 'inline-flex', alignItems: 'center', gap: 'var(--s-2)' }}>
                <StatusDot tone={linkTone} pulse={linkTone === 'orange'} />
                {linkText}
              </span>
            }
          />
          <Stat label="Switch time" value={formatMs(diag?.switchLatencyMsP50)} />
          <Stat label="Up for" value={diag ? formatUptime(diag.uptimeS) : '—'} />
        </div>
      </section>

      {controller ? (
        <Section title="Quick tips">
          <Row
            icon={<Gauge size={16} strokeWidth={1.6} />}
            title="Switch computers"
            subtitle="Works from anywhere, even while another computer is in use."
            trailing={settings && <KeyCombo keys={settings.switchKey.keys} os={os} />}
          />
          <Row
            icon={<MousePointer2 size={16} strokeWidth={1.6} />}
            title="Cross the screen edge"
            subtitle="Move the pointer past the side of one screen to reach the next."
            trailing={
              <Badge tone={settings?.edgeSwitching ? 'green' : 'neutral'}>
                {settings?.edgeSwitching ? 'On' : 'Off'}
              </Badge>
            }
          />
          <Row
            icon={<Undo2 size={16} strokeWidth={1.6} />}
            title="Come back"
            subtitle="Jump straight back to this computer."
            trailing={
              <KeyCombo keys={settings?.returnHomeKey?.keys ?? settings?.switchKey.keys ?? []} os={os} />
            }
          />
        </Section>
      ) : (
        // A controlled computer has no shortcuts of its own: the controller's
        // keyboard decides when it is used, and Release control hands it back.
        <Section title="Quick tips">
          <Row
            icon={<MousePointer2 size={16} strokeWidth={1.6} />}
            title={`Use it from ${controllerName}`}
            subtitle={`Move ${controllerName}’s pointer off its screen edge, or press its switch shortcut, to work here.`}
          />
          <Row
            icon={<Undo2 size={16} strokeWidth={1.6} />}
            title="Take your computer back"
            subtitle={`While ${controllerName} is working here, select Release control at the top of this page.`}
          />
        </Section>
      )}
    </div>
  );
}
