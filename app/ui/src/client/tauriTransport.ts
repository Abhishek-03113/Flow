import { FlowCommandError } from './errors';
import type { Transport, TransportHandlers, WireEvent } from './transport';

export const TAURI_EVENT = 'flow://event';
export const isTauriRuntime = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * Production transport: one `flow_command(command, payload)` invoke and the
 * `flow://event` stream. After subscribing it sends `resync`, which makes the
 * engine re-emit every state event (the Tauri equivalent of "WebSocket sends
 * all events on connect").
 */
export class TauriTransport implements Transport {
  readonly kind = 'tauri' as const;
  private unlisten: (() => void) | null = null;
  /** Bumped by every connect/close so a stale async connect (React StrictMode) undoes itself. */
  private generation = 0;

  connect(h: TransportHandlers): void {
    const generation = ++this.generation;
    h.onStatus('connecting');
    void (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const un = await listen<WireEvent>(TAURI_EVENT, (e) => h.onEvent(e.payload));
        if (generation !== this.generation) return un();
        this.unlisten = un;
        await this.send('resync');
        h.onStatus('open');
      } catch (e) {
        h.onStatus('closed', e instanceof Error ? e.message : String(e));
      }
    })();
  }

  async send(command: string, payload: unknown = {}): Promise<unknown> {
    const { invoke } = await import('@tauri-apps/api/core');
    try {
      return await invoke('flow_command', { command, payload });
    } catch (e) {
      const err = e as { code?: string; message?: string } | string;
      if (typeof err === 'object' && err !== null && typeof err.code === 'string') {
        throw new FlowCommandError(err.code, err.message ?? err.code);
      }
      throw new FlowCommandError('transport_error', String(err));
    }
  }

  close(): void {
    this.generation++;
    this.unlisten?.();
    this.unlisten = null;
  }
}
