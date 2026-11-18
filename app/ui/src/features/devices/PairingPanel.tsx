import { Check, TriangleAlert } from 'lucide-react';
import { useEffect, useRef, useState, type FormEvent } from 'react';
import type { PairingCandidate, Role } from '../../client/types';
import { osName } from '../../copy';
import { Badge, Button, Row, Section, Spinner } from '@flow/design-system';
import { useFlow } from '../../state/FlowProvider';
import { DeviceGlyph } from '../DeviceGlyph';
import type { usePairingFlow } from './usePairingFlow';

type Flow = ReturnType<typeof usePairingFlow>;

/** With nothing found after this long, discovery is probably blocked: offer the checklist and an address. */
export const FALLBACK_AFTER_MS = 10_000;

/** True once `active` has stayed true for `ms`. */
function useHeldFor(active: boolean, ms: number): boolean {
  const [held, setHeld] = useState(false);
  useEffect(() => {
    setHeld(false);
    if (!active) return;
    const t = setTimeout(() => setHeld(true), ms);
    return () => clearTimeout(t);
  }, [active, ms]);
  return held;
}

const sameRoleNote = (myRole: Role) =>
  myRole === 'controller'
    ? 'Also shares its keyboard and mouse. On one of the two, choose “Control this computer from another one” in General.'
    : 'Also set to be controlled. One of the two has to share its keyboard and mouse: change it in General.';

/** Type the other computer's address: works where discovery can't (no broadcast on the network). */
function AddressForm({ flow, focus }: { flow: Flow; focus?: boolean }) {
  const [address, setAddress] = useState('');
  const input = useRef<HTMLInputElement>(null);
  // Only when the person asked for this form; never on its own.
  useEffect(() => {
    if (focus) input.current?.focus();
  }, [focus]);
  const mine = useFlow().diagnostics?.localAddresses ?? [];
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (address.trim()) void flow.pairAddress(address.trim());
  };
  return (
    <form className="fl-address" onSubmit={submit} data-testid="pair-by-address">
      <label className="fl-field">
        <span className="fl-field__label">The other computer’s address, as Flow shows it there</span>
        <span className="fl-address__row">
          <input
            className="fl-input"
            value={address}
            onChange={(e) => setAddress(e.target.value)}
            placeholder="192.168.1.20"
            ref={input}
            autoComplete="off"
            spellCheck={false}
            inputMode="url"
          />
          <Button type="submit" variant="prominent" disabled={!address.trim()}>
            Connect
          </Button>
        </span>
      </label>
      <p className="t-footnote t-secondary" data-testid="local-addresses">
        {mine.length
          ? `This computer’s address: ${mine.join(', ')}`
          : 'This computer has no network address right now. Check that it’s connected to Wi-Fi or Ethernet.'}
      </p>
    </form>
  );
}

function CandidateRow({
  c,
  myRole,
  onPair,
}: {
  c: PairingCandidate;
  myRole: Role;
  onPair(id: string): void;
}) {
  const sameRole = c.role !== undefined && c.role === myRole;
  return (
    <Row
      icon={<DeviceGlyph os={c.os} />}
      title={c.name}
      subtitle={sameRole ? sameRoleNote(myRole) : osName(c.os)}
      trailing={
        sameRole ? (
          <Badge tone="orange">Same role</Badge>
        ) : (
          <Button
            size="small"
            variant="prominent"
            aria-label={`Connect to ${c.name}`}
            onClick={() => onPair(c.id)}
          >
            Connect
          </Button>
        )
      }
    />
  );
}

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
  const [byAddress, setByAddress] = useState(false);
  const looking = !display || display.stage === 'searching' || display.stage === 'found';
  const candidates = display?.stage === 'found' ? display.candidates : [];
  const slow = useHeldFor(looking && candidates.length === 0, FALLBACK_AFTER_MS);

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
        {display.code ? (
          <>
            <p className="t-callout t-secondary">Check that this code matches on {display.targetName}.</p>
            <div className="fl-code" aria-label={`Verification code ${display.code}`}>
              {display.code}
            </div>
          </>
        ) : (
          <p className="t-callout t-secondary">Connecting to {display.targetName}…</p>
        )}
        <div
          style={{ display: 'flex', alignItems: 'center', gap: 'var(--s-2)' }}
          className="t-callout t-secondary"
        >
          <Spinner size={14} /> Waiting for {display.targetName} to accept…
        </div>
        <Button onClick={() => void flow.cancel()}>Cancel</Button>
      </div>
    );
  }

  const usable = candidates.filter((c) => !(c.role !== undefined && c.role === myRole));
  return (
    <div style={{ display: 'grid', gap: 'var(--s-4)' }}>
      {candidates.length > 0 ? (
        <div data-testid="pairing-found">
          <Section
            title="Nearby computers"
            footer={
              usable.length === 0
                ? 'Both computers have the same role, so they can’t pair. Change the role of one of them first.'
                : myRole === 'controller'
                  ? 'The computer you pick will be controlled by this one.'
                  : 'The computer you pick will share its keyboard and mouse with this one.'
            }
          >
            {candidates.map((c) => (
              <CandidateRow key={c.id} c={c} myRole={myRole} onPair={(id) => void flow.pair(id)} />
            ))}
          </Section>
        </div>
      ) : (
        <div className="fl-center" data-testid="pairing-searching">
          <Spinner size={28} label="Searching" />
          <h3 className="t-title3">Looking for computers…</h3>
          <p className="t-callout t-secondary" style={{ maxWidth: 340 }}>
            Open Flow on the other computer and keep it on the same network. It appears here by itself.
          </p>
        </div>
      )}

      {slow && (
        <div data-testid="pairing-troubleshoot">
          <Section title="Not showing up?">
            <Row
              title="Same network"
              subtitle="Both computers on the same Wi-Fi or wired network, no guest Wi-Fi."
            />
            <Row
              title="Flow open on both"
              subtitle="And its setup finished there, including the firewall step."
            />
            <Row
              title="Connect by address"
              subtitle="Some networks don’t let computers find each other. Type the address Flow shows on the other computer."
            />
          </Section>
        </div>
      )}

      {slow || byAddress ? (
        <AddressForm flow={flow} focus={byAddress} />
      ) : (
        <div className="fl-center">
          <Button variant="plain" onClick={() => setByAddress(true)}>
            Connect by Address…
          </Button>
        </div>
      )}
    </div>
  );
}
