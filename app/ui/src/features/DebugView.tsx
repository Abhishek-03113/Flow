import { useFlow, useClient } from '../state/FlowProvider';

/** Developer-only engine inspector at `#/debug`. */
export function DebugView() {
  const state = useFlow();
  const client = useClient();
  return (
    <main style={{ padding: 24, font: '13px/1.5 ui-monospace, monospace' }}>
      <h1 style={{ font: '600 22px system-ui' }}>Engine inspector</h1>
      <p>
        transport: <b>{client.transport.kind}</b> · connection: <b>{state.connection}</b> · loaded:{' '}
        <b>{String(state.loaded)}</b> · link: <b>{state.linkState}</b>
      </p>
      <pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(state, null, 2)}</pre>
    </main>
  );
}
