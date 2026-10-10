import { FlowCommandError } from './errors';
import type { Transport, TransportHandlers, WireEvent } from './transport';

interface Ack {
  id: string;
  ok: boolean;
  data?: unknown;
  error?: { code: string; message: string };
}

type WebSocketCtor = new (url: string, protocols?: string | string[]) => WebSocket;

export interface WebSocketTransportOptions {
  url: string;
  /** Daemon IPC token, presented as the `Sec-WebSocket-Protocol` subprotocol. */
  token: string;
  /** Injection point for tests. */
  WebSocketImpl?: WebSocketCtor;
  /** Reconnect backoff in ms (default 1000 → 5000). Set 0 to disable. */
  reconnectMs?: number;
}

/** Speaks `docs/contracts/daemon-ipc.md`: `{id,command,payload}` → `{id,ok,error?}` + `{event,payload}` pushes. */
export class WebSocketTransport implements Transport {
  readonly kind = 'websocket' as const;
  private ws: WebSocket | null = null;
  private handlers: TransportHandlers | null = null;
  private pending = new Map<string, { resolve(v: unknown): void; reject(e: Error): void }>();
  private seq = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private stopped = false;
  private attempts = 0;

  constructor(private readonly opts: WebSocketTransportOptions) {}

  connect(h: TransportHandlers): void {
    this.handlers = h;
    this.open();
  }

  private open(): void {
    const Impl = this.opts.WebSocketImpl ?? WebSocket;
    this.handlers?.onStatus('connecting');
    const ws = new Impl(this.opts.url, this.opts.token);
    this.ws = ws;
    ws.onopen = () => {
      this.attempts = 0;
      this.handlers?.onStatus('open');
    };
    ws.onmessage = (m: MessageEvent) => this.onMessage(String(m.data));
    ws.onerror = () => undefined; // `onclose` always follows
    ws.onclose = () => {
      this.ws = null;
      const err = new FlowCommandError('connection_closed', 'The connection to Flow was lost.');
      this.pending.forEach((p) => p.reject(err));
      this.pending.clear();
      this.handlers?.onStatus('closed', 'Cannot reach the Flow service.');
      this.scheduleReconnect();
    };
  }

  private scheduleReconnect(): void {
    const base = this.opts.reconnectMs ?? 1000;
    if (this.stopped || base === 0) return;
    const delay = Math.min(base * 2 ** Math.min(this.attempts++, 3), 5000);
    this.timer = setTimeout(() => this.open(), delay);
  }

  private onMessage(raw: string): void {
    let msg: Ack | WireEvent;
    try {
      msg = JSON.parse(raw) as Ack | WireEvent;
    } catch {
      return;
    }
    if ('event' in msg) return this.handlers?.onEvent(msg);
    const p = this.pending.get(msg.id);
    if (!p) return;
    this.pending.delete(msg.id);
    if (msg.ok) p.resolve(msg.data);
    else p.reject(new FlowCommandError(msg.error?.code ?? 'unknown', msg.error?.message ?? 'Command failed'));
  }

  send(command: string, payload: unknown = {}): Promise<unknown> {
    const ws = this.ws;
    if (!ws || ws.readyState !== 1) {
      return Promise.reject(new FlowCommandError('connection_closed', 'Not connected to Flow.'));
    }
    const id = `req-${++this.seq}`;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      ws.send(JSON.stringify({ id, command, payload }));
    });
  }

  close(): void {
    this.stopped = true;
    if (this.timer) clearTimeout(this.timer);
    this.ws?.close();
  }
}
