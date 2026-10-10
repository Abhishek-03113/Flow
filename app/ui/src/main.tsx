import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { createClient } from './client/createClient';
import { MockTransport } from './client/mockTransport';

const selection = createClient();
if (selection.client.transport instanceof MockTransport) {
  (window as unknown as { __flowMock?: unknown }).__flowMock = selection.client.transport.engine.simulate;
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App selection={selection} />
  </StrictMode>,
);
