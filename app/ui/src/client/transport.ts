/** An unsolicited engine push: `{ event, payload }` (snake_case payload). */
export interface WireEvent {
  event: string;
  payload: unknown;
}

export type TransportStatus = 'connecting' | 'open' | 'closed';

export interface TransportHandlers {
  onEvent(event: WireEvent): void;
  onStatus(status: TransportStatus, error?: string): void;
}

/**
 * How `FlowClient` reaches an engine. Three implementations: Tauri (IPC),
 * WebSocket (daemon contract) and Mock (in-memory simulation).
 * `send` resolves with the ack's optional `result` and rejects with `FlowCommandError`.
 */
export interface Transport {
  readonly kind: 'tauri' | 'websocket' | 'mock';
  connect(handlers: TransportHandlers): void;
  send(command: string, payload?: unknown): Promise<unknown>;
  close(): void;
}
