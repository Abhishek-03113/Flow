import { FlowCommandError } from './errors';
import { MockEngine, type MockOptions } from './mockEngine';
import type { Transport, TransportHandlers } from './transport';

/** `Transport` over an in-memory `MockEngine`, with a little latency for realism. */
export class MockTransport implements Transport {
  readonly kind = 'mock' as const;
  readonly engine: MockEngine;
  private connected = false;

  constructor(
    opts: MockOptions & { latencyMs?: number } = {},
    private readonly latencyMs = opts.latencyMs ?? 40,
  ) {
    this.engine = new MockEngine(opts);
  }

  connect(h: TransportHandlers): void {
    h.onStatus('connecting');
    queueMicrotask(() => {
      this.connected = true;
      h.onStatus('open');
      this.engine.attach((event, payload) => h.onEvent({ event, payload }));
    });
  }

  send(command: string, payload: unknown = {}): Promise<unknown> {
    if (!this.connected) return Promise.reject(new FlowCommandError('connection_closed', 'Not connected.'));
    const run = () => this.engine.handle(command, payload);
    // Zero latency stays on the microtask queue so fake-timer tests don't hang.
    if (this.latencyMs <= 0) {
      return new Promise((resolve, reject) => {
        try {
          resolve(run());
        } catch (e) {
          reject(e);
        }
      });
    }
    return new Promise((resolve, reject) => {
      setTimeout(() => {
        try {
          resolve(run());
        } catch (e) {
          reject(e);
        }
      }, this.latencyMs);
    });
  }

  close(): void {
    this.connected = false;
    this.engine.detach();
  }
}
