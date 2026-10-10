import { Download } from 'lucide-react';
import { useState } from 'react';
import { Alert, Button, Row, Section, Toggle } from '@flow/design-system';
import { formatUptime } from '../../copy';
import { useClient, useFlow } from '../../state/FlowProvider';
import { useAction } from '../../state/useAction';
import { useToast } from '@flow/design-system';

function Stat({ value, label, hint }: { value: string; label: string; hint?: string }) {
  return (
    <div className="fl-card" data-testid={`stat-${label}`}>
      <div className="fl-stat__value">{value}</div>
      <div className="fl-stat__label">{label}</div>
      {hint && <div className="t-footnote t-secondary">{hint}</div>}
    </div>
  );
}

const n = (v: number) => v.toLocaleString('en-US');

export function DiagnosticsView() {
  const s = useFlow();
  const client = useClient();
  const toast = useToast();
  const { run } = useAction();
  const [confirmReset, setConfirmReset] = useState(false);
  const d = s.diagnostics;
  const settings = s.settings;
  if (!settings) return null;
  const total = d ? d.eventsForwarded + d.eventsDropped : 0;
  const dropPct = d && total > 0 ? ((d.eventsDropped / total) * 100).toFixed(2) : '0';

  return (
    <div data-testid="diagnostics">
      <section aria-labelledby="perf-h" className="fl-section">
        <h2 id="perf-h" className="fl-section__title">
          Performance
        </h2>
        <div className="fl-grid" style={{ gridTemplateColumns: 'repeat(3, 1fr)' }}>
          <Stat value={d ? `${d.switchLatencyMsP50} ms` : '—'} label="Switch time (typical)" hint="Median" />
          <Stat
            value={d ? `${d.switchLatencyMsP95} ms` : '—'}
            label="Switch time (slowest)"
            hint="95th percentile"
          />
          <Stat value={d ? formatUptime(d.uptimeS) : '—'} label="Running for" />
          <Stat value={d ? n(d.eventsForwarded) : '—'} label="Inputs sent" />
          <Stat
            value={d ? n(d.eventsDropped) : '—'}
            label="Inputs dropped"
            hint={`${dropPct}% of all inputs`}
          />
        </div>
      </section>

      <Section
        title="Troubleshooting"
        footer="Detailed logs help when you contact support. They can include app activity, never what you type."
      >
        <Row
          title="Detailed logging"
          subtitle="Record extra information about what Flow is doing."
          trailing={
            <Toggle
              label="Detailed logging"
              checked={settings.debugLogging}
              onChange={(v) => void run(() => client.updateSettings({ debugLogging: v }))}
            />
          }
        />
        <Row
          title="Export diagnostics"
          subtitle="Save a report you can send to support."
          trailing={
            <Button
              size="small"
              icon={<Download size={13} strokeWidth={1.7} />}
              onClick={() =>
                void run(async () => {
                  const r = await client.exportDiagnostics();
                  toast.show(r?.path ? `Saved to ${r.path}` : 'Diagnostics exported');
                })
              }
            >
              Export…
            </Button>
          }
        />
      </Section>

      <Section title="Reset" footer="Resets shortcuts and preferences. Your paired computers stay.">
        <Row
          title="Reset all settings"
          trailing={
            <Button size="small" tone="destructive" onClick={() => setConfirmReset(true)}>
              Reset…
            </Button>
          }
        />
      </Section>

      <Alert
        open={confirmReset}
        title="Reset all settings?"
        message="Shortcuts and preferences go back to their defaults. This can’t be undone."
        onClose={() => setConfirmReset(false)}
        actions={[
          {
            label: 'Reset Settings',
            variant: 'destructive-prominent',
            onClick: () => {
              setConfirmReset(false);
              void run(() => client.resetSettings(), 'Settings were reset');
            },
          },
          { label: 'Cancel', onClick: () => setConfirmReset(false), autoFocus: true },
        ]}
      />
    </div>
  );
}
