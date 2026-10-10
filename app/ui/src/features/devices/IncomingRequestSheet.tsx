import { osName } from '../../copy';
import { Button, Sheet } from '@flow/design-system';
import { useAction } from '../../state/useAction';
import { useClient, useFlow } from '../../state/FlowProvider';
import { DeviceGlyph } from '../DeviceGlyph';

/** Global prompt: another computer wants to pair with this one. Declining is the safe default (Esc). */
export function IncomingRequestSheet() {
  const { incomingPairingRequest: req } = useFlow();
  const client = useClient();
  const { run, busy } = useAction();
  if (!req) return null;
  const answer = (d: 'accept' | 'reject') => run(() => client.respondToPairingRequest(req.requestId, d));
  return (
    <Sheet
      open
      title={`Add ${req.deviceName}?`}
      onClose={() => void answer('reject')}
      data-testid="incoming-request"
      footer={
        <>
          <Button onClick={() => void answer('reject')} disabled={busy}>
            Decline
          </Button>
          <Button variant="prominent" onClick={() => void answer('accept')} loading={busy} data-autofocus>
            Accept
          </Button>
        </>
      }
    >
      <div className="fl-center">
        <div className="fl-glyph-circle fl-glyph-circle--accent">
          <DeviceGlyph os={req.deviceOs} size={26} />
        </div>
        <p className="t-callout t-secondary">
          A {osName(req.deviceOs)} computer wants to connect to this one. Only accept if you recognise it and
          the code below matches.
        </p>
        <div className="fl-code" style={{ fontSize: 22 }} aria-label={`Verification code ${req.fingerprint}`}>
          {req.fingerprint}
        </div>
        {req.address && <p className="t-footnote t-secondary">{req.address}</p>}
        <p className="t-footnote t-secondary">This request expires after 30 seconds.</p>
      </div>
    </Sheet>
  );
}
