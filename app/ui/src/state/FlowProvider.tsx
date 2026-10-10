import { createContext, useContext, useEffect, useSyncExternalStore, type ReactNode } from 'react';
import type { FlowClient } from '../client/flowClient';
import type { FlowState } from '../client/types';

/**
 * State management: `FlowClient` is already a tiny external store (engine
 * pushes are the single source of truth), so React reads it with
 * `useSyncExternalStore` through a context. No store library: there is no
 * client-only state worth normalising, and the snapshot is replaced
 * immutably on each push so selectors stay trivially correct.
 */
const Ctx = createContext<FlowClient | null>(null);

export function FlowProvider({ client, children }: { client: FlowClient; children: ReactNode }) {
  useEffect(() => {
    client.connect();
    return () => client.close();
  }, [client]);
  return <Ctx.Provider value={client}>{children}</Ctx.Provider>;
}

export function useClient(): FlowClient {
  const c = useContext(Ctx);
  if (!c) throw new Error('useClient must be used inside <FlowProvider>');
  return c;
}

export function useFlow(): FlowState {
  const c = useClient();
  return useSyncExternalStore(c.subscribe, c.getSnapshot, c.getSnapshot);
}
