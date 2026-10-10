/**
 * `flow-harness up` from Node: starts virtual engines (each with a real
 * control server and test hooks), waits for the manifest, drives the
 * harness's stdin control (`disconnect NAME` / `reconnect NAME`) and stops
 * the whole process group on teardown. `DaemonClient` speaks the daemon IPC
 * contract directly (docs/contracts/daemon-ipc.md) for test hooks and checks.
 */
import { spawn, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

export const REPO = path.resolve(import.meta.dirname, '../../..');
const TARGET = process.env.CARGO_TARGET_DIR ?? path.join(REPO, 'target');
export const HARNESS_BIN = process.env.FLOW_HARNESS_BIN ?? path.join(TARGET, 'release', 'flow-harness');

export type Os = 'macos' | 'windows' | 'linux';
export type Role = 'controller' | 'controlled';

export interface DeviceSpec {
  name: string;
  os: 'mac' | 'win' | 'lin';
  role: Role;
}

export interface WorldSpec {
  devices: DeviceSpec[];
  /** Pair and link every controller with every controlled device before the test. */
  paired?: boolean;
  security?: 'secure' | 'insecure';
}

export interface ManifestEntry {
  name: string;
  os: Os;
  role: Role;
  ws_url: string;
  token: string;
}

/** One running `flow-harness up`. */
export class Harness {
  private stdoutBuffer = '';
  private replies: { resolve(v: { ok: boolean; error?: string }): void }[] = [];
  private exited: Promise<number | null>;
  private exitCode: number | null | undefined = undefined;
  readonly log: string[] = [];

  private constructor(
    private readonly child: ChildProcess,
    readonly manifest: ManifestEntry[],
    readonly dir: string,
  ) {
    this.exited = new Promise((resolve) => child.once('exit', (code) => resolve(code)));
    void this.exited.then((code) => (this.exitCode = code));
  }

  static async start(spec: WorldSpec, timeoutMs = 60_000): Promise<Harness> {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'flow-live-'));
    const manifestPath = path.join(dir, 'manifest.json');
    const devices = spec.devices.map((d) => `${d.name}:${d.os}:${d.role}`).join(',');
    const args = ['up', '--devices', devices, '--manifest', manifestPath, '--stop-on-eof'];
    if (spec.paired) args.push('--paired');
    if (spec.security) args.push('--security', spec.security);
    // Own process group (detached) so teardown can signal everything it started.
    const child = spawn(HARNESS_BIN, args, {
      detached: true,
      stdio: ['pipe', 'pipe', 'pipe'],
      env: { ...process.env, RUST_LOG: process.env.RUST_LOG ?? 'warn' },
    });
    const log: string[] = [];
    let early: number | null | undefined;
    child.once('exit', (code) => (early = code));
    child.stderr!.setEncoding('utf8');
    child.stderr!.on('data', (chunk: string) => log.push(chunk));

    const started = Date.now();
    while (!fs.existsSync(manifestPath)) {
      if (early !== undefined) {
        throw new Error(`flow-harness up exited (${early}) before it was ready:\n${log.join('')}`);
      }
      if (Date.now() - started > timeoutMs) {
        killGroup(child, 'SIGKILL');
        throw new Error(`flow-harness up not ready after ${timeoutMs} ms:\n${log.join('')}`);
      }
      await new Promise((r) => setTimeout(r, 50));
    }
    const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8')) as ManifestEntry[];
    const h = new Harness(child, manifest, dir);
    h.log.push(...log);
    child.stderr!.on('data', (chunk: string) => h.log.push(chunk));
    child.stdout!.setEncoding('utf8');
    child.stdout!.on('data', (chunk: string) => h.onStdout(chunk));
    return h;
  }

  entry(name: string): ManifestEntry {
    const e = this.manifest.find((m) => m.name === name);
    if (!e) throw new Error(`no device ${name} in ${this.manifest.map((m) => m.name).join(', ')}`);
    return e;
  }

  private onStdout(chunk: string): void {
    this.stdoutBuffer += chunk;
    let nl: number;
    while ((nl = this.stdoutBuffer.indexOf('\n')) >= 0) {
      const line = this.stdoutBuffer.slice(0, nl).trim();
      this.stdoutBuffer = this.stdoutBuffer.slice(nl + 1);
      if (!line.startsWith('{')) continue;
      const v = JSON.parse(line) as { control?: string; ok?: boolean; error?: string };
      if (v.control === undefined) continue; // the manifest lines
      this.replies.shift()?.resolve({ ok: v.ok === true, error: v.error });
    }
  }

  private async control(line: string): Promise<void> {
    if (this.exitCode !== undefined) throw new Error('flow-harness is not running');
    const reply = new Promise<{ ok: boolean; error?: string }>((resolve) => this.replies.push({ resolve }));
    this.child.stdin!.write(`${line}\n`);
    const r = await reply;
    if (!r.ok) throw new Error(`${line}: ${r.error}`);
  }

  /** Kills that device's engine abruptly (its peers see the link drop, its page loses the socket). */
  disconnect = (name: string) => this.control(`disconnect ${name}`);
  /** Restarts it on the same data and control port; paired links come back on their own. */
  reconnect = (name: string) => this.control(`reconnect ${name}`);

  /** SIGINT, then SIGKILL for the whole group if it does not exit promptly. */
  async stop(): Promise<void> {
    if (this.exitCode === undefined) {
      killGroup(this.child, 'SIGINT');
      const t = setTimeout(() => killGroup(this.child, 'SIGKILL'), 5000);
      await this.exited;
      clearTimeout(t);
    }
    fs.rmSync(this.dir, { recursive: true, force: true });
  }
}

function killGroup(child: ChildProcess, signal: NodeJS.Signals): void {
  if (child.pid === undefined) return;
  try {
    process.kill(-child.pid, signal);
  } catch {
    /* already gone */
  }
}

// --- the control API, straight from Node -------------------------------------

/** Wire `InputEvent` (`{"Keyboard":{"KeyDown":{...}}}`, `{"Mouse":{"Move":{...}}}`). */
export type InputEvent =
  | { Keyboard: { KeyDown: KeyFields } | { KeyUp: KeyFields } }
  | { Mouse: Record<string, Record<string, unknown>> };
interface KeyFields {
  key: string;
  modifiers: string[];
  timestamp_ms: number;
}

export interface VirtualIo {
  capturing: boolean;
  suppressed: boolean;
  delivered_locally: InputEvent[];
  injected: InputEvent[];
  warps: [number, number][];
}

export interface WireDevice {
  id: string;
  name: string;
  os: Os;
  state: string;
  role: Role | null;
  link_error: string | null;
  is_local: boolean;
}

type Pending = { resolve(v: unknown): void; reject(e: Error): void };

/** A typed client of one daemon's control server; keeps the latest value of every event. */
export class DaemonClient {
  private seq = 0;
  private pending = new Map<string, Pending>();
  /** Latest payload per event name. */
  readonly state = new Map<string, unknown>();
  /** Every event in arrival order (name only), for "was it sent" checks. */
  readonly seen: { event: string; payload: unknown }[] = [];
  private clock = 1_000_000;

  private constructor(private readonly ws: WebSocket) {
    ws.onmessage = (m) => this.onMessage(String(m.data));
    ws.onclose = () => {
      const err = new Error('daemon connection closed');
      this.pending.forEach((p) => p.reject(err));
      this.pending.clear();
    };
  }

  static connect(entry: ManifestEntry): Promise<DaemonClient> {
    return new Promise((resolve, reject) => {
      const ws = new WebSocket(entry.ws_url, entry.token);
      ws.onopen = () => resolve(new DaemonClient(ws));
      ws.onerror = () => reject(new Error(`cannot connect to ${entry.name} at ${entry.ws_url}`));
    });
  }

  private onMessage(raw: string): void {
    const msg = JSON.parse(raw) as {
      id?: string;
      ok?: boolean;
      result?: unknown;
      error?: { code: string; message: string };
      event?: string;
      payload?: unknown;
    };
    if (msg.event) {
      this.state.set(msg.event, msg.payload);
      this.seen.push({ event: msg.event, payload: msg.payload });
      return;
    }
    const p = msg.id ? this.pending.get(msg.id) : undefined;
    if (!p) return;
    this.pending.delete(msg.id!);
    if (msg.ok) p.resolve(msg.result);
    else p.reject(Object.assign(new Error(`${msg.error?.code}: ${msg.error?.message}`), msg.error));
  }

  /** Any contract command; resolves with `result` (or undefined for a plain ack). */
  call<T = unknown>(command: string, payload: unknown = null): Promise<T> {
    const id = `t-${++this.seq}`;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.ws.send(JSON.stringify({ id, command, payload }));
    });
  }

  close(): void {
    this.ws.close();
  }

  // --- test hooks ---
  inject = (event: InputEvent) => this.call('debug_inject_input', event);
  virtualIo = (clear = false) => this.call<VirtualIo>('debug_virtual_io', clear ? { clear: true } : null);
  /** Reads, then empties, the virtual OS's logs. */
  clearIo = () => this.virtualIo(true);

  private ts(): number {
    return ++this.clock;
  }
  async keyDown(key: string, modifiers: string[] = []): Promise<void> {
    await this.inject({ Keyboard: { KeyDown: { key, modifiers, timestamp_ms: this.ts() } } });
  }
  async keyUp(key: string, modifiers: string[] = []): Promise<void> {
    await this.inject({ Keyboard: { KeyUp: { key, modifiers, timestamp_ms: this.ts() } } });
  }
  async tap(key: string): Promise<void> {
    await this.keyDown(key);
    await this.keyUp(key);
  }
  /** Presses every key of a chord, then releases them in reverse (wire key names). */
  async chord(keys: string[]): Promise<void> {
    for (const k of keys) await this.keyDown(k);
    for (const k of [...keys].reverse()) await this.keyUp(k);
  }
  async move(dx: number, dy: number): Promise<void> {
    await this.inject({ Mouse: { Move: { dx, dy, timestamp_ms: this.ts() } } });
  }

  // --- state as the engine sees it ---
  devices = (): WireDevice[] => (this.state.get('devices_changed') as WireDevice[] | undefined) ?? [];
  idOf(name: string): string {
    const d = this.devices().find((x) => x.name === name);
    if (!d) throw new Error(`${name} is not among ${this.devices().map((x) => x.name)}`);
    return d.id;
  }
  controlState = () =>
    this.state.get('control_state_changed') as
      | { role: Role; active_screen: string; controller_device_id: string | null; entered: boolean }
      | undefined;
  settings = () => this.state.get('settings_changed') as Record<string, unknown> | undefined;
  linkState = () => this.state.get('link_state_changed') as string | undefined;
}

/** Wire key names a virtual OS saw, e.g. `down A`, `up A`, ignoring moves. */
export function keyLog(events: InputEvent[]): string[] {
  const out: string[] = [];
  for (const e of events) {
    if (!('Keyboard' in e)) continue;
    const k = e.Keyboard as Record<string, KeyFields>;
    if (k.KeyDown) out.push(`down ${k.KeyDown.key}`);
    if (k.KeyUp) out.push(`up ${k.KeyUp.key}`);
  }
  return out;
}
