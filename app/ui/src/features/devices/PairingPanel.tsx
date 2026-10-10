import { Check, TriangleAlert } from 'lucide-react';
import type { Role } from '../../client/types';
import { osName } from '../../copy';
import { Button, EmptyState, Row, Section, Spinner } from '@flow/design-system';
import { DeviceGlyph } from '../DeviceGlyph';
import type { usePairingFlow } from './usePairingFlow';

type Flow = ReturnType<typeof usePairingFlow>;

/** The pairing stages as one reusable body (sheet in Devices, page in onboarding). */
export function PairingPanel({
  flow,
  myRole,
  connectedTo,
}: {
  flow: Flow;
  myRole: Role;
  /** Set when this computer got paired the other way round (it accepted a request). */
  connectedTo?: string;
}) {
  const { display } = flow;

  if (display?.stage === 'paired' || (connectedTo && display?.stage !== 'requesting')) {
    return (
      <div className="fl-center" data-testid="pairing-paired">
        <div className="fl-glyph-circle fl-glyph-circle--green">
          <Check size={28} strokeWidth={1.8} aria-hidden />
        </div>
        <h3 className="t-title3">
          Connected to {display?.stage === 'paired' ? display.targetName : connectedTo}
        </h3>
        <p className="t-callout t-secondary">You can now switch between your computers.</p>
      </div>
    );
  }

  if (display?.stage === 'failed') {
    return (
      <div className="fl-center" data-testid="pairing-failed">
        <div className="fl-glyph-circle fl-glyph-circle--red">
          <TriangleAlert size={26} strokeWidth={1.6} aria-hidden />
        </div>
        <h3 className="t-title3">Couldn’t connect</h3>
        <p className="t-callout t-secondary">{display.error ?? 'The other computer didn’t respond.'}</p>
        <Button onClick={() => void flow.start()}>Try Again</Button>
      </div>
    );
  }

  if (display?.stage === 'requesting') {
    return (
      <div className="fl-center" data-testid="pairing-requesting">
        <p className="t-callout t-secondary">Check that this code matches on {display.targetName}.</p>
        <div className="fl-code" aria-label={`Verification code ${display.code ?? ''}`}>
          {display.code ?? '···'}
        </div>
        <div
          style={{ display: 'flex', alignItems: 'center', gap: 'var(--s-2)' }}
          className="t-callout t-secondary"
        >
          <Spinner size={14} /> Waiting for {display.targetName} to accept…
        </div>
      </div>
    );
  }

  if (display?.stage === 'found') {
    if (display.candidates.length === 0) {
      return (
        <EmptyState
          title="No computers found"
          actions={<Button onClick={() => void flow.start()}>Search Again</Button>}
        >
          Open Flow on the other computer and make sure it’s on the same network.
        </EmptyState>
      );
    }
    return (
      <div data-testid="pairing-found">
        <Section
          title="Nearby computers"
          footer={
            myRole === 'controller'
              ? 'The computer you pick will be controlled by this one.'
              : 'The computer you pick will share its keyboard and mouse with this one.'
          }
        >
          {display.candidates.map((c) => (
            <Row
              key={c.id}
              icon={<DeviceGlyph os={c.os} />}
              title={c.name}
              subtitle={osName(c.os)}
              trailing={
                <Button
                  size="small"
                  variant="prominent"
                  aria-label={`Connect to ${c.name}`}
                  onClick={() => void flow.pair(c.id)}
                >
                  Connect
                </Button>
              }
            />
          ))}
        </Section>
      </div>
    );
  }

  return (
    <div className="fl-center" data-testid="pairing-searching">
      <Spinner size={28} label="Searching" />
      <h3 className="t-title3">Looking for computers…</h3>
      <p className="t-callout t-secondary" style={{ maxWidth: 320 }}>
        Open Flow on the other computer and keep it on the same network.
      </p>
    </div>
  );
}
