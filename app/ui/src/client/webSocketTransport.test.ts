import { describe, expect, it, vi } from 'vitest';
import { FlowClient } from './flowClient';
import { WebSocketTransport } from './webSocketTransport';

class FakeWS {
  static last: FakeWS;
  readyState = 0;
  sent: string[] = [];
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((m: { data: string }) => void) | null = null;
  constructor(
    public url: string,
    public protocols?: string | string[],
  ) {
    FakeWS.last = this;
  }
  send(d: string) {
    this.sent.push(d);
  }
  close() {
    this.readyState = 3;
    this.onclose?.();
  }
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
  push(o: unknown) {
    this.onmessage?.({ data: JSON.stringify(o) });
  }
}

const make = () => {
  const t = new WebSocketTransport({
    url: 'ws://127.0.0.1:47823',
    token: 'secret-token',
    WebSocketImpl: FakeWS as unknown as ConstructorParameters<typeof WebSocket>[0] extends never
      ? never
      : typeof WebSocket,
    reconnectMs: 0,
  });
  const client = new FlowClient(t);
  client.connect();
  return { t, client, ws: FakeWS.last };
};

describe('WebSocketTransport', () => {
  it('presents the token as the Sec-WebSocket-Protocol subprotocol', () => {
    const { ws } = make();
    expect(ws.url).toBe('ws://127.0.0.1:47823');
    expect(ws.protocols).toBe('secret-token');
  });

  it('sends {id,command,payload} with snake_case payloads and resolves on ok ack', async () => {
    const { client, ws } = make();
    ws.open();
    const p = client.removeDevice('d2');
    const sent = JSON.parse(ws.sent[0]!);
    expect(sent).toMatchObject({ command: 'remove_device', payload: { device_id: 'd2' } });
    ws.push({ id: sent.id, ok: true });
    await expect(p).resolves.toBeUndefined();
  });

  it('rejects with the daemon error code and surfaces the ack result', async () => {
    const { client, ws } = make();
    ws.open();
    const bad = client.switchTo('x');
    ws.push({
      id: JSON.parse(ws.sent[0]!).id,
      ok: false,
      error: { code: 'device_not_found', message: 'no device x' },
    });
    await expect(bad).rejects.toMatchObject({ code: 'device_not_found', message: 'no device x' });
    const good = client.exportDiagnostics();
    ws.push({ id: JSON.parse(ws.sent[1]!).id, ok: true, result: { path: '/tmp/x' } });
    await expect(good).resolves.toEqual({ path: '/tmp/x' });
  });

  it('routes pushes into client state and reports closed connections', async () => {
    const { client, ws } = make();
    ws.open();
    ws.push({ event: 'link_state_changed', payload: 'reconnecting' });
    expect(client.getSnapshot().linkState).toBe('reconnecting');
    expect(client.getSnapshot().connection).toBe('open');
    const pending = client.retryConnection();
    ws.close();
    await expect(pending).rejects.toMatchObject({ code: 'connection_closed' });
    expect(client.getSnapshot().connection).toBe('closed');
    await expect(client.retryConnection()).rejects.toMatchObject({ code: 'connection_closed' });
  });

  it('reconnects with backoff when enabled', () => {
    vi.useFakeTimers();
    const t = new WebSocketTransport({
      url: 'ws://x',
      token: 't',
      WebSocketImpl: FakeWS as never,
      reconnectMs: 100,
    });
    const client = new FlowClient(t);
    client.connect();
    const first = FakeWS.last;
    first.open();
    first.close();
    vi.advanceTimersByTime(150);
    expect(FakeWS.last).not.toBe(first);
    vi.useRealTimers();
  });
});
