import { render } from '@testing-library/react';
import { App } from '../App';
import type { ClientSelection } from '../client/createClient';
import { FlowClient } from '../client/flowClient';
import { MockTransport } from '../client/mockTransport';
import type { HostOs } from '../client/types';

/** Renders the whole app against an instant, tick-free mock engine. */
export function renderApp(scenario: string, hash = '/home', os: HostOs = 'macos') {
  window.location.hash = hash;
  const transport = new MockTransport({ scenario, os, ticks: false, latencyMs: 0, timeScale: 0 });
  const selection: ClientSelection = { client: new FlowClient(transport), kind: 'mock', scenario };
  const utils = render(<App selection={selection} />);
  return { ...utils, client: selection.client, engine: transport.engine };
}
