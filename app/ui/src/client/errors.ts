/** Mirrors the daemon's `{ code, message }` error payload. */
export class FlowCommandError extends Error {
  readonly code: string;
  constructor(code: string, message: string) {
    super(message);
    this.name = 'FlowCommandError';
    this.code = code;
  }
}

export const isFlowCommandError = (e: unknown): e is FlowCommandError => e instanceof FlowCommandError;
