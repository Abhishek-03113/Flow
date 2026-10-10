import { useCallback, useEffect, useState } from 'react';
import type { PairingSession } from '../../client/types';
import { friendlyError } from '../../copy';
import { useToast } from '../../design';
import { useClient, useFlow } from '../../state/FlowProvider';

/**
 * Wraps the engine's pairing session. The engine returns to `idle` ~1.6s after
 * `paired`/`failed`, which would flash the outcome away, so the last terminal
 * session is held until the user dismisses it.
 */
export function usePairingFlow() {
  const client = useClient();
  const toast = useToast();
  const { pairingSession } = useFlow();
  const [held, setHeld] = useState<PairingSession | null>(null);
  const [starting, setStarting] = useState(false);

  useEffect(() => {
    if (pairingSession.stage === 'paired' || pairingSession.stage === 'failed') setHeld(pairingSession);
    if (pairingSession.stage !== 'idle') setStarting(false);
  }, [pairingSession]);

  const display = pairingSession.stage !== 'idle' ? pairingSession : held;

  const start = useCallback(async () => {
    setHeld(null);
    setStarting(true);
    try {
      await client.startPairing();
    } catch (e) {
      setStarting(false);
      toast.show(friendlyError(e), 'error');
    }
  }, [client, toast]);

  const stop = useCallback(async () => {
    setHeld(null);
    setStarting(false);
    if (client.getSnapshot().pairingSession.stage !== 'idle')
      await client.cancelPairing().catch(() => undefined);
  }, [client]);

  const pair = useCallback(
    (id: string) => client.pairWithCandidate(id).catch((e) => toast.show(friendlyError(e), 'error')),
    [client, toast],
  );

  return { display, searching: starting && !display, start, stop, pair };
}
