import { useEffect } from 'react';
import { Button, Sheet } from '../../design';
import { useFlow } from '../../state/FlowProvider';
import { myRole } from '../../state/selectors';
import { useUi } from '../../state/ui';
import { PairingPanel } from './PairingPanel';
import { usePairingFlow } from './usePairingFlow';

/** "Add Computer" sheet, openable from anywhere through `useUi().openPairing()`. */
export function PairingSheet() {
  const { pairingOpen, closePairing } = useUi();
  const state = useFlow();
  const flow = usePairingFlow();
  const { start, stop } = flow;

  useEffect(() => {
    if (pairingOpen) void start();
    else void stop();
    // start/stop identities are stable per client
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pairingOpen]);

  const stage = flow.display?.stage;
  const terminal = stage === 'paired' || stage === 'failed';
  return (
    <Sheet
      open={pairingOpen}
      title="Add Computer"
      onClose={closePairing}
      data-testid="pairing-sheet"
      footer={
        terminal ? (
          <Button variant="prominent" onClick={closePairing}>
            {stage === 'paired' ? 'Done' : 'Close'}
          </Button>
        ) : (
          <Button onClick={closePairing}>Cancel</Button>
        )
      }
    >
      <PairingPanel flow={flow} myRole={myRole(state)} />
    </Sheet>
  );
}
