import { FlowClient } from './flowClient';
import { MockTransport } from './mockTransport';
import { scenarioHasCompletedOnboarding, SCENARIOS } from './mockScenarios';
import { TauriTransport, isTauriRuntime } from './tauriTransport';
import type { Transport } from './transport';
import type { HostOs } from './types';
import { WebSocketTransport } from './webSocketTransport';

export interface ClientSelection {
  client: FlowClient;
  kind: Transport['kind'];
  scenario: string | null;
}

const OS: HostOs[] = ['macos', 'windows', 'linux'];

/**
 * Tauri runtime → tauri; `?daemon=ws://127.0.0.1:PORT&token=…` → websocket;
 * otherwise → mock (`?scenario=`, `?os=`, `?fast=1`).
 */
export function createClient(search: string = window.location.search): ClientSelection {
  const q = new URLSearchParams(search);
  if (isTauriRuntime())
    return { client: new FlowClient(new TauriTransport()), kind: 'tauri', scenario: null };
  const daemon = q.get('daemon');
  if (daemon) {
    const t = new WebSocketTransport({ url: daemon, token: q.get('token') ?? '' });
    return { client: new FlowClient(t), kind: 'websocket', scenario: null };
  }
  const scenario = q.get('scenario') ?? 'two-devices';
  const os = q.get('os') as HostOs | null;
  const fast = q.get('fast') === '1';
  const t = new MockTransport({
    scenario: SCENARIOS.includes(scenario as never) ? scenario : 'two-devices',
    os: os && OS.includes(os) ? os : 'macos',
    timeScale: fast ? 0.05 : 1,
    latencyMs: fast ? 0 : 40,
  });
  return { client: new FlowClient(t), kind: 'mock', scenario };
}

export { scenarioHasCompletedOnboarding };
