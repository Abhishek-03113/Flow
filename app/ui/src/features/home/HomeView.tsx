import { Gauge, MousePointer2, Undo2 } from 'lucide-react';
import type { ReactNode } from 'react';
import { friendlyError, formatUptime } from '../../copy';
import { Badge, KeyCombo, Row, Section, StatusDot, useToast } from '@flow/design-system';
import { useClient, useFlow } from '../../state/FlowProvider';
import { localDevice, localOs, myRole, remoteDevices } from '../../state/selectors';
import { ArrangementEditor } from './ArrangementEditor';
import { StatusCard } from './StatusCard';

const LINK_TEXT = {
  connected: ['Healthy', 'green'],
  connecting: ['Connecting', 'orange'],
  reconnecting: ['Reconnecting', 'orange'],
  disconnected: ['Offline', 'gray'],
  error: ['Needs attention', 'red'],
  permission_required: ['Needs permission', 'red'],
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
  const [linkText, linkTone] = LINK_TEXT[s.linkState];
  const activeScreen = s.controlState?.activeScreen ?? 'local';
  const activeId = controller
    ? activeScreen === 'local'
      ? (me?.id ?? null)
      : activeScreen
    : activeScreen === 'local'
      ? (me?.id ?? null)
      : activeScreen;
  const hasLayout = s.screenLayout.screens.length > 1;
  const diag = s.diagnostics;
  const settings = s.settings;

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
              layout={s.screenLayout}
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
                : 'The computer that shares its keyboard and mouse sets up the arrangement.'}
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
          <Stat label="Switch time" value={diag ? `${diag.switchLatencyMsP50} ms` : '—'} />
          <Stat label="Up for" value={diag ? formatUptime(diag.uptimeS) : '—'} />
        </div>
      </section>

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
    </div>
  );
}
