import type { ClientSelection } from './client/createClient';
import { DebugView } from './features/DebugView';
import { FlowProvider } from './state/FlowProvider';

export function App({ selection }: { selection: ClientSelection }) {
  return (
    <FlowProvider client={selection.client}>
      <DebugView />
    </FlowProvider>
  );
}
