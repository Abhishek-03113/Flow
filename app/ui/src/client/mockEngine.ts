import { FlowCommandError } from './errors';
import { fromWire, toWire } from './mapper';
import {
  candidatePool,
  defaultSettings,
  layoutFor,
  LOCAL_DEVICE_ID,
  sampleIncomingRequest,
  seedScenario,
  type EngineDevice,
  type EngineState,
} from './mockScenarios';
import { bindingProblem, remapKey, sameKeys } from './shortcutRules';
import type {
  HostOs,
  Keymap,
  LinkState,
  RemapRule,
  Role,
  ScreenLayout,
  SettingsPatch,
  SwitchKeyBinding,
} from './types';

export interface MockOptions {
  scenario?: string;
  /** OS of "this computer". Defaults to macOS (the HIG reference platform). */
  os?: HostOs;
  /** Multiplies every simulated delay. `0` = as fast as the event loop allows. */
  timeScale?: number;
  /** Emit a diagnostics tick every second while open. Disable in unit tests. */
  ticks?: boolean;
}

type Emit = (event: string, payload: unknown) => void;
type Json = Record<string, unknown>;

/** A few keys of each embedded keymap (`config/keymaps/*.toml`), in the engine's file shape. */
const KEYMAPS: Record<HostOs, Keymap> = {
  macos: {
    version: 1,
    os: 'macos',
    keys: [
      { name: 'A', code: 0x00 },
      { name: 'B', code: 0x0b },
      { name: 'COMMAND', code: 0x37 },
      { name: 'CONTROL', code: 0x3b },
      { name: 'F13', code: 0x69 },
    ],
  },
  windows: {
    version: 1,
    os: 'windows',
    keys: [
      { name: 'A', code: 0x41 },
      { name: 'B', code: 0x42 },
      { name: 'CONTROL', code: 0xa2 },
      { name: 'COMMAND', code: 0x5b },
      { name: 'SCROLLLOCK', code: 0x91 },
    ],
  },
  linux: {
    version: 1,
    os: 'linux',
    keys: [
      { name: 'A', code: 30 },
      { name: 'B', code: 48 },
      { name: 'CONTROL', code: 29 },
      { name: 'COMMAND', code: 125 },
      { name: 'SCROLLLOCK', code: 70 },
    ],
  },
};

const err = (code: string, message: string) => new FlowCommandError(code, message);

/**
 * In-memory simulation of the engine's contract (0.1.3 + 0.2.0): same commands,
 * same preconditions and error codes, same events. All state is held in the
 * domain (camelCase) form and put on the wire with `toWire`, so the mock also
 * exercises the real mapper.
 */
export class MockEngine {
  private s: EngineState;
  private emit: Emit = () => undefined;
  private timers = new Set<ReturnType<typeof setTimeout>>();
  private interval: ReturnType<typeof setInterval> | null = null;
  private latencies: number[] = [8, 9, 11, 12, 14, 24];
  private nextId = 10;
  private readonly scale: number;
  readonly os: HostOs;

  constructor(private readonly opts: MockOptions = {}) {
    this.os = opts.os ?? 'macos';
    this.scale = opts.timeScale ?? 1;
    this.s = seedScenario(opts.scenario ?? 'two-devices', this.os);
  }

  get state(): Readonly<EngineState> {
    return this.s;
  }

  // --- lifecycle -----------------------------------------------------------
  attach(emit: Emit): void {
    this.emit = emit;
    this.resync();
    if (this.opts.ticks !== false) {
      this.interval = setInterval(() => this.tick(), 1000);
    }
    if (this.opts.scenario === 'pairing-request') {
      this.later(900, () => this.simulate.incomingPairingRequest());
    }
  }

  detach(): void {
    this.timers.forEach(clearTimeout);
    this.timers.clear();
    if (this.interval) clearInterval(this.interval);
    this.emit = () => undefined;
  }

  private later(ms: number, fn: () => void): void {
    const t = setTimeout(() => {
      this.timers.delete(t);
      fn();
    }, ms * this.scale);
    this.timers.add(t);
  }

  private push(event: string, value: unknown): void {
    this.emit(event, toWire(value));
  }

  /** Sends every state event (the "sent on connect" set). */
  resync(): void {
    this.pushDevices();
    this.push('link_state_changed', this.s.linkState);
    this.push('pairing_session_changed', this.s.pairingSession);
    this.push('settings_changed', this.s.settings);
    this.push('permission_changed', this.s.permission);
    this.push('incoming_pairing_request_changed', this.s.incomingPairingRequest);
    this.push('control_state_changed', this.s.controlState);
    this.push('screen_layout_changed', this.s.screenLayout);
    this.push('diagnostics_changed', this.s.diagnostics);
    this.push('remap_rules_changed', this.s.remapRules);
  }

  /** Like the engine, tells each device whether it is this computer. */
  private pushDevices = () =>
    this.push(
      'devices_changed',
      this.s.devices.map((d) => ({ ...d, isLocal: d.id === LOCAL_DEVICE_ID })),
    );
  private pushControl = () => this.push('control_state_changed', this.s.controlState);
  private pushSettings = () => this.push('settings_changed', this.s.settings);
  private pushLink = () => this.push('link_state_changed', this.s.linkState);
  private pushPairing = () => this.push('pairing_session_changed', this.s.pairingSession);
  private pushLayout = () => this.push('screen_layout_changed', this.s.screenLayout);

  private tick(): void {
    const d = this.s.diagnostics;
    const remote = this.s.controlState.role === 'controller' && this.s.controlState.activeScreen !== 'local';
    this.s.diagnostics = {
      ...d,
      uptimeS: d.uptimeS + 1,
      eventsForwarded: d.eventsForwarded + (remote ? 40 + Math.floor(Math.random() * 80) : 0),
    };
    this.push('diagnostics_changed', this.s.diagnostics);
  }

  // --- command dispatcher ---------------------------------------------------
  /** `payload` arrives in wire (snake_case) form, exactly as a transport would deliver it. */
  handle(command: string, rawPayload: unknown): unknown {
    const p = fromWire<Json>(rawPayload ?? {});
    switch (command) {
      case 'resync':
        return void this.resync();
      case 'switch_to':
        return this.switchTo(String(p.target));
      case 'switch_active_device': // compatibility alias
        return this.switchTo(String(p.deviceId));
      case 'release_control':
        return this.releaseControl();
      case 'remove_device':
        return this.removeDevice(String(p.deviceId));
      case 'start_pairing':
        return this.startPairing();
      case 'cancel_pairing':
        return this.cancelPairing();
      case 'pair_with_candidate':
        return this.pairWithCandidate(String(p.candidateId));
      case 'respond_to_pairing_request':
        return this.respondToPairingRequest(String(p.requestId), p.decision === 'accept');
      case 'set_switch_key':
        return this.setSwitchKey(p as unknown as { label: string; keys: string[] });
      case 'update_settings':
        return this.updateSettings(p as SettingsPatch);
      case 'reset_settings':
        return this.resetSettings();
      case 'request_permission':
        return this.requestPermission();
      case 'retry_connection':
        return this.retryConnection();
      case 'recheck_permissions':
        return undefined; // the mock has no OS to re-probe
      case 'cancel_reconnect':
        return this.cancelReconnect();
      case 'open_permission_settings':
        return this.openPermissionSettings(String(p.name));
      case 'set_screen_layout':
        return this.setScreenLayout(p.layout as ScreenLayout);
      case 'set_role':
        return this.setRole(p.role as Role);
      case 'get_keymap':
        return KEYMAPS[(p.os as HostOs) ?? this.os] ?? KEYMAPS[this.os];
      case 'set_remap_rules':
        return this.setRemapRules(p.rules as RemapRule[]);
      case 'reveal_config_dir':
        return { path: this.s.settings.keymapOverridesPath };
      case 'export_diagnostics':
        return { path: '~/Downloads/flow-diagnostics.txt' };
      default:
        throw err('unknown_command', `unrecognized command: ${command}`);
    }
  }

  private device(id: string): EngineDevice {
    const d = this.s.devices.find((x) => x.id === id);
    if (!d) throw err('device_not_found', `no device ${id}`);
    return d;
  }

  // --- switching & control ---------------------------------------------------
  private switchTo(target: string): void {
    if (this.s.settings.role !== 'controller') {
      throw err('not_controller', 'Only the computer sharing its keyboard and mouse can switch.');
    }
    if (target !== 'local') {
      const d = this.device(target);
      if (d.id === LOCAL_DEVICE_ID) target = 'local';
      else if (d.linkError || !['inactive', 'connected', 'active'].includes(d.state)) {
        throw err('device_not_switchable', `${d.name} is not reachable`);
      }
    }
    const started = performance.now();
    this.s.devices = this.s.devices.map((d) => ({
      ...d,
      state:
        d.id === target || (target === 'local' && d.id === LOCAL_DEVICE_ID)
          ? 'active'
          : d.state === 'active'
            ? 'inactive'
            : d.state,
      lastSeenMs: d.id === target ? Date.now() : d.lastSeenMs,
    }));
    this.s.controlState = { ...this.s.controlState, activeScreen: target };
    this.latencies.push(Math.round(8 + (performance.now() - started) + Math.random() * 12));
    this.updateLatency();
    this.pushDevices();
    this.pushControl();
  }

  private updateLatency(): void {
    const sorted = [...this.latencies].sort((a, b) => a - b);
    const q = (f: number) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * f))]!;
    this.s.diagnostics = { ...this.s.diagnostics, switchLatencyMsP50: q(0.5), switchLatencyMsP95: q(0.95) };
    this.push('diagnostics_changed', this.s.diagnostics);
  }

  private releaseControl(): void {
    const c = this.s.controlState;
    if (c.role !== 'controlled' || !c.controllerDeviceId) {
      throw err('not_controlled', 'Nothing is controlling this computer right now.');
    }
    // The controller decides; it returns to its own screen.
    this.s.controlState = { ...c, entered: false };
    this.pushControl();
  }

  private removeDevice(id: string): void {
    const d = this.device(id);
    if (d.id === LOCAL_DEVICE_ID) throw err('device_not_removable', 'This computer cannot be removed.');
    if (this.s.controlState.activeScreen === id) {
      this.switchTo('local');
    }
    this.s.devices = this.s.devices.filter((x) => x.id !== id);
    this.s.screenLayout = { screens: this.s.screenLayout.screens.filter((x) => x.deviceId !== id) };
    this.pushDevices();
    this.pushLayout();
  }

  // --- pairing ---------------------------------------------------------------
  private startPairing(): void {
    if (this.s.pairingSession.stage !== 'idle')
      throw err('pairing_in_progress', 'Already looking for computers.');
    this.s.pairingSession = { stage: 'searching', candidates: [], targetName: null, error: null, code: null };
    this.pushPairing();
    this.later(1200, () => {
      const known = new Set(this.s.devices.map((d) => d.name));
      this.s.pairingSession = {
        ...this.s.pairingSession,
        stage: 'found',
        candidates: candidatePool.filter((c) => !known.has(c.name)),
      };
      this.pushPairing();
    });
  }

  private resetPairing(): void {
    this.s.pairingSession = { stage: 'idle', candidates: [], targetName: null, error: null, code: null };
    this.pushPairing();
  }

  private cancelPairing(): void {
    if (this.s.pairingSession.stage === 'idle') throw err('pairing_not_active', 'No pairing in progress.');
    this.timers.forEach(clearTimeout);
    this.timers.clear();
    this.resetPairing();
  }

  private pairWithCandidate(id: string): void {
    const sess = this.s.pairingSession;
    if (sess.stage !== 'found') throw err('pairing_not_ready', 'Not ready to pair yet.');
    const cand = sess.candidates.find((c) => c.id === id);
    if (!cand) throw err('candidate_not_found', `no candidate ${id}`);
    this.s.pairingSession = { ...sess, stage: 'requesting', targetName: cand.name, code: '482 913' };
    this.pushPairing();
    this.later(1500, () => {
      if (cand.os === 'linux') {
        // Demonstrates the failure state: the other computer says no.
        this.s.pairingSession = {
          ...this.s.pairingSession,
          stage: 'failed',
          code: null,
          error: `${cand.name} didn't accept the request.`,
        };
      } else {
        this.addDevice({ name: cand.name, os: cand.os });
        this.s.pairingSession = { ...this.s.pairingSession, stage: 'paired', code: null };
      }
      this.pushPairing();
      this.later(1600, () => this.resetPairing());
    });
  }

  private addDevice(info: { name: string; os: HostOs }): void {
    const role: Role = this.s.settings.role === 'controller' ? 'controlled' : 'controller';
    const dev: EngineDevice = {
      id: `d${this.nextId++}`,
      name: info.name,
      os: info.os,
      state: 'inactive',
      role,
      screen: { width: 1920, height: 1080 },
      lastSeenMs: Date.now(),
      linkError: null,
    };
    this.s.devices = [...this.s.devices, dev];
    const right = Math.max(0, ...this.s.screenLayout.screens.map((r) => r.x + r.width));
    this.s.screenLayout = {
      screens: [
        ...this.s.screenLayout.screens,
        { deviceId: dev.id, x: right, y: 0, width: 1920, height: 1080 },
      ],
    };
    this.pushDevices();
    this.pushLayout();
  }

  private respondToPairingRequest(id: string, accept: boolean): void {
    const req = this.s.incomingPairingRequest;
    if (!req || req.requestId !== id)
      throw err('pairing_request_not_found', 'That request is no longer pending.');
    this.s.incomingPairingRequest = null;
    this.push('incoming_pairing_request_changed', null);
    if (accept) this.addDevice({ name: req.deviceName, os: req.deviceOs });
  }

  // --- settings ---------------------------------------------------------------
  /** Like the engine's `check_shortcuts`: usable bindings, and return-home ≠ switch. */
  private checkShortcuts(p: { switchKey?: SwitchKeyBinding; returnHomeKey?: SwitchKeyBinding | null }): void {
    for (const b of [p.switchKey, p.returnHomeKey]) {
      if (!b) continue;
      const problem = bindingProblem(b.keys ?? []);
      if (problem === 'empty') throw err('invalid_switch_key', 'A shortcut needs at least one key.');
      if (problem) throw err('switch_key_needs_modifier', 'A typing key needs Ctrl, Alt or Meta.');
    }
    if (p.switchKey === undefined && !p.returnHomeKey) return;
    const sw = p.switchKey ?? this.s.settings.switchKey;
    const home = p.returnHomeKey !== undefined ? p.returnHomeKey : this.s.settings.returnHomeKey;
    if (home && sameKeys(home.keys, sw.keys)) {
      throw err('switch_key_conflict', 'The return-home shortcut is the same as the switch shortcut.');
    }
  }

  private setSwitchKey(b: { label: string; keys: string[] }): void {
    this.checkShortcuts({ switchKey: b });
    this.s.settings = { ...this.s.settings, switchKey: { label: b.label, keys: b.keys } };
    this.pushSettings();
  }

  private updateSettings(patch: SettingsPatch): void {
    const { role, ...rest } = patch;
    this.checkShortcuts(rest);
    if (
      rest.edgeSwitchDelayMs !== undefined &&
      (rest.edgeSwitchDelayMs < 0 || rest.edgeSwitchDelayMs > 2000)
    ) {
      throw err('invalid_payload', 'Delay must be between 0 and 2000 ms.');
    }
    this.s.settings = { ...this.s.settings, ...rest };
    this.pushSettings();
    if (role && role !== this.s.settings.role) this.setRole(role);
  }

  private resetSettings(): void {
    const keepRole = this.s.settings.role;
    this.s.settings = { ...defaultSettings(), role: keepRole };
    this.pushSettings();
  }

  private setRole(role: Role): void {
    if (role !== 'controller' && role !== 'controlled')
      throw err('invalid_payload', `unknown role ${String(role)}`);
    this.s.settings = { ...this.s.settings, role };
    // Re-evaluate links: two computers with the same role cannot work together.
    this.s.devices = this.s.devices.map((d) => {
      if (d.id === LOCAL_DEVICE_ID) return { ...d, role };
      if (d.role === role && !d.linkError) return { ...d, state: 'error', linkError: 'role_conflict' };
      if (d.role !== role && d.linkError === 'role_conflict')
        return { ...d, state: 'inactive', linkError: null };
      return d;
    });
    const controller =
      role === 'controlled' ? this.s.devices.find((d) => d.role === 'controller' && !d.linkError) : null;
    this.s.controlState = {
      role,
      activeScreen: 'local',
      controllerDeviceId: controller?.id ?? null,
      entered: false,
    };
    if (role === 'controller') {
      this.s.devices = this.s.devices.map((d) =>
        d.id === LOCAL_DEVICE_ID
          ? { ...d, state: 'active' }
          : d.state === 'active'
            ? { ...d, state: 'inactive' }
            : d,
      );
    }
    this.pushSettings();
    this.pushDevices();
    this.pushControl();
    this.syncLinkWithDevices();
  }

  /** Like the engine: a refused link with no other live link is `error` (not `reconnecting`). */
  private syncLinkWithDevices(): void {
    const remote = this.s.devices.filter((d) => d.id !== LOCAL_DEVICE_ID);
    const live = remote.some((d) => !d.linkError && ['inactive', 'active', 'connected'].includes(d.state));
    if (this.s.linkState === 'error' && !this.s.devices.some((d) => d.linkError)) {
      this.s.linkState = 'connected';
      this.pushLink();
    } else if (this.s.linkState !== 'error' && !live && remote.some((d) => d.linkError === 'role_conflict')) {
      this.s.linkState = 'error';
      this.pushLink();
    }
  }

  private requestPermission(): void {
    if (this.s.permission.granted) throw err('permission_already_granted', 'Already allowed.');
    // The OS prompt is out of our hands; simulate the user flipping the switch.
    this.later(2500, () => this.simulate.grantPermission());
  }

  private cancelReconnect(): void {
    if (this.s.linkState !== 'reconnecting') {
      throw err('not_reconnecting', `Link is ${this.s.linkState}; nothing to cancel.`);
    }
    // The engine stops redialling until the user retries.
    this.setLink('disconnected');
  }

  private openPermissionSettings(name: string): { url: string | null } {
    const item = this.s.permission.items.find((i) => i.name === name);
    if (!item) throw err('permission_not_found', `no permission named ${name}`);
    const pane = name === 'Accessibility' ? 'Privacy_Accessibility' : 'Privacy_ListenEvent';
    return {
      url: this.os === 'macos' ? `x-apple.systempreferences:com.apple.preference.security?${pane}` : null,
    };
  }

  private retryConnection(): void {
    if (this.s.linkState !== 'disconnected' && this.s.linkState !== 'error') {
      throw err('link_not_recoverable', `Link is ${this.s.linkState}; nothing to retry.`);
    }
    this.setLink('connecting');
    this.later(1200, () => {
      this.s.devices = this.s.devices.map((d) =>
        d.state === 'disconnected' || (d.state === 'error' && !d.linkError) ? { ...d, state: 'inactive' } : d,
      );
      this.pushDevices();
      this.setLink('connected');
    });
  }

  private setLink(state: LinkState): void {
    this.s.linkState = state;
    this.pushLink();
  }

  private setScreenLayout(layout: ScreenLayout): void {
    const ids = new Set(this.s.devices.map((d) => d.id));
    for (const r of layout?.screens ?? []) {
      if (!ids.has(r.deviceId)) throw err('device_not_found', `no device ${r.deviceId}`);
      if (!(r.width > 0 && r.height > 0)) throw err('invalid_layout', 'Screens need a size.');
    }
    const rs = layout.screens;
    for (let i = 0; i < rs.length; i++) {
      for (let j = i + 1; j < rs.length; j++) {
        const a = rs[i]!;
        const b = rs[j]!;
        if (a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height) {
          throw err('invalid_layout', 'Screens cannot overlap.');
        }
      }
    }
    this.s.screenLayout = layout;
    this.pushLayout();
  }

  private setRemapRules(rules: RemapRule[]): void {
    for (const r of rules) {
      if (!r.from || !r.to) throw err('invalid_remap_rule', 'Both keys are required.');
      // Like the engine: a modifier maps to a modifier, a key to a key.
      const mod = (t: string) => ['Ctrl', 'Alt', 'Shift', 'Meta'].includes(t);
      if (mod(r.from) !== mod(r.to))
        throw err('invalid_remap_rule', 'cannot map a modifier to a key or the reverse');
    }
    // Like the engine: only the first rule for a key and computer would apply.
    const seen = new Set<string>();
    for (const r of rules) {
      const k = `${remapKey(r.from)}|${r.whenTargetOs ?? '*'}`;
      if (seen.has(k)) throw err('duplicate_remap_rule', `more than one rule for ${r.from}`);
      seen.add(k);
    }
    this.s.remapRules = rules;
    this.push('remap_rules_changed', rules);
  }

  // --- test / demo hooks (exposed as window.__flowMock) -----------------------
  readonly simulate = {
    incomingPairingRequest: (partial: Partial<typeof sampleIncomingRequest> = {}) => {
      this.s.incomingPairingRequest = { ...sampleIncomingRequest, ...partial };
      this.push('incoming_pairing_request_changed', this.s.incomingPairingRequest);
    },
    withdrawPairingRequest: () => {
      this.s.incomingPairingRequest = null;
      this.push('incoming_pairing_request_changed', null);
    },
    linkState: (state: LinkState) => this.setLink(state),
    grantPermission: () => {
      if (this.s.permission.granted) return;
      this.s.permission = {
        ...this.s.permission,
        granted: true,
        items: this.s.permission.items.map((i) => ({ ...i, granted: true, howToFix: null })),
      };
      this.push('permission_changed', this.s.permission);
      if (this.s.linkState === 'permission_required') this.setLink('connected');
    },
    /** The remote controller moves input to/from this (controlled) computer. */
    controllerEnters: (enter: boolean) => {
      const c = this.s.controlState;
      if (c.role !== 'controlled' || !c.controllerDeviceId) return;
      this.s.controlState = { ...c, entered: enter };
      this.pushControl();
    },
    deviceState: (id: string, state: EngineDevice['state']) => {
      this.s.devices = this.s.devices.map((d) => (d.id === id ? { ...d, state } : d));
      this.pushDevices();
    },
    layout: () => layoutFor(this.s.devices),
  };
}
