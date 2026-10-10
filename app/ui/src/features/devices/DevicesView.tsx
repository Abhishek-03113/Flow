import { ArrowLeftRight, Laptop, Plus, Trash2, TriangleAlert } from 'lucide-react';
import { useState } from 'react';
import type { Device } from '../../client/types';
import { formatRelative, linkErrorCopy, osName, roleLabel } from '../../copy';
import { Alert, Badge, Button, EmptyState, Row, Section, Tooltip } from '../../design';
import { useClient, useFlow } from '../../state/FlowProvider';
import { isReachable, localDevice, myRole, remoteDevices } from '../../state/selectors';
import { useAction } from '../../state/useAction';
import { useNow } from '../../state/useNow';
import { useUi } from '../../state/ui';
import { DeviceGlyph } from '../DeviceGlyph';

function statusOf(d: Device, now: number): { text: string; badge: ReactBadge } {
  if (d.linkError) return { text: 'Needs attention', badge: ['red', 'Needs attention'] };
  switch (d.state) {
    case 'active':
      return { text: 'Active', badge: ['accent', 'Active'] };
    case 'inactive':
    case 'connected':
      return { text: 'Connected', badge: ['green', 'Connected'] };
    case 'pairing':
      return { text: 'Pairing…', badge: ['orange', 'Pairing'] };
    case 'error':
      return { text: 'Needs attention', badge: ['red', 'Needs attention'] };
    case 'disconnected':
      return { text: `Offline · seen ${formatRelative(d.lastSeenMs, now)}`, badge: ['neutral', 'Offline'] };
  }
}
type ReactBadge = ['neutral' | 'accent' | 'green' | 'orange' | 'red', string];

export function DevicesToolbarAction() {
  const { openPairing } = useUi();
  return (
    <Button icon={<Plus size={14} strokeWidth={1.8} />} onClick={openPairing}>
      Add Computer…
    </Button>
  );
}

export function DevicesView() {
  const s = useFlow();
  const client = useClient();
  const { navigate, openPairing } = useUi();
  const { run, busy } = useAction();
  const now = useNow();
  const [forgetting, setForgetting] = useState<Device | null>(null);
  const me = localDevice(s);
  const role = myRole(s);
  const others = remoteDevices(s);

  return (
    <div data-testid="devices">
      {me && (
        <Section title="This computer">
          <Row
            icon={<DeviceGlyph os={me.os} />}
            iconTone="accent"
            title={me.name}
            subtitle={`${osName(me.os)} · ${roleLabel(role)}${me.screen ? ` · ${me.screen.width}×${me.screen.height}` : ''}`}
            trailing={
              <Button size="small" onClick={() => navigate('/general')}>
                Role…
              </Button>
            }
          />
        </Section>
      )}

      {others.length === 0 ? (
        <div className="fl-card">
          <EmptyState
            art={<Laptop size={44} strokeWidth={1.2} />}
            title="No other computers yet"
            actions={
              <Button variant="prominent" onClick={openPairing}>
                Add Computer…
              </Button>
            }
          >
            Add a computer to start sharing. Both need Flow open and to be on the same network.
          </EmptyState>
        </div>
      ) : (
        <Section title="Other computers">
          {others.map((d) => {
            const st = statusOf(d, now);
            const err = d.linkError ? linkErrorCopy(d.linkError, d, role) : null;
            const canSwitch = role === 'controller' && isReachable(d) && d.state !== 'active';
            return (
              <Row
                key={d.id}
                data-testid={`device-${d.id}`}
                icon={<DeviceGlyph os={d.os} />}
                iconTone={
                  d.state === 'active' ? 'accent' : d.linkError || d.state === 'error' ? 'red' : 'neutral'
                }
                title={
                  <>
                    {d.name}
                    <Badge tone={st.badge[0]}>{st.badge[1]}</Badge>
                  </>
                }
                subtitle={`${osName(d.os)} · ${roleLabel(d.role)}${st.text === st.badge[1] ? '' : ` · ${st.text}`}`}
                trailing={
                  <>
                    {canSwitch && (
                      <Button
                        size="small"
                        icon={<ArrowLeftRight size={13} strokeWidth={1.8} />}
                        disabled={busy}
                        onClick={() => void run(() => client.switchTo(d.id))}
                      >
                        Switch
                      </Button>
                    )}
                    <Tooltip label="Forget this computer">
                      <Button
                        size="small"
                        aria-label={`Forget ${d.name}`}
                        icon={<Trash2 size={14} strokeWidth={1.6} />}
                        onClick={() => setForgetting(d)}
                      />
                    </Tooltip>
                  </>
                }
              >
                {err && (
                  <div className="fl-notice fl-notice--red" role="alert" data-testid="link-error">
                    <TriangleAlert
                      size={16}
                      strokeWidth={1.8}
                      color="var(--red)"
                      aria-hidden
                      style={{ marginTop: 1 }}
                    />
                    <div className="fl-notice__body">
                      <p className="fl-notice__title">{err.title}</p>
                      <p className="t-callout t-secondary">{err.detail}</p>
                    </div>
                    {err.action === 'role' && (
                      <Button size="small" onClick={() => navigate('/general')}>
                        Change Role…
                      </Button>
                    )}
                  </div>
                )}
              </Row>
            );
          })}
        </Section>
      )}

      <Alert
        open={forgetting !== null}
        title={`Forget ${forgetting?.name ?? ''}?`}
        message="Flow will stop sharing with this computer. You can add it again later."
        onClose={() => setForgetting(null)}
        actions={[
          {
            label: 'Forget Computer',
            variant: 'destructive-prominent',
            onClick: () => {
              const d = forgetting!;
              setForgetting(null);
              void run(() => client.removeDevice(d.id), `${d.name} was removed`);
            },
          },
          { label: 'Cancel', onClick: () => setForgetting(null), autoFocus: true },
        ]}
      />
    </div>
  );
}
