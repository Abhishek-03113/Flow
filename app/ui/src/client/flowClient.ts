import { decodeEvent, type FlowEvent } from './events';
import { toWire } from './mapper';
import type { Transport, TransportStatus } from './transport';
import type {
  ControlState,
  FlowState,
  HostOs,
  KeymapEntry,
  RemapRule,
  Role,
  ScreenLayout,
  SettingsPatch,
  SwitchKeyBinding,
} from './types';

export const initialState: FlowState = {
  connection: 'connecting',
  connectionError: null,
  loaded: false,
  devices: [],
  linkState: 'connecting',
  pairingSession: { stage: 'idle', candidates: [], targetName: null, error: null, code: null },
  settings: null,
  permission: null,
  incomingPairingRequest: null,
  controlState: null,
  screenLayout: { screens: [] },
  diagnostics: null,
  remapRules: [],
};

/** Snapshot events: once all have arrived, the UI can render without spinners. */
const SNAPSHOT: FlowEvent['type'][] = [
  'devices_changed',
  'link_state_changed',
  'settings_changed',
  'permission_changed',
  'control_state_changed',
];

export function reduce(state: FlowState, e: FlowEvent): FlowState {
  switch (e.type) {
    case 'devices_changed':
      return { ...state, devices: e.devices };
    case 'link_state_changed':
      return { ...state, linkState: e.linkState };
    case 'pairing_session_changed':
      return { ...state, pairingSession: e.session };
    case 'settings_changed':
      return { ...state, settings: e.settings };
    case 'permission_changed':
      return { ...state, permission: e.permission };
    case 'incoming_pairing_request_changed':
      return { ...state, incomingPairingRequest: e.request };
    case 'control_state_changed':
      return { ...state, controlState: e.controlState };
    case 'screen_layout_changed':
      return { ...state, screenLayout: e.layout };
    case 'diagnostics_changed':
      return { ...state, diagnostics: e.diagnostics };
    case 'remap_rules_changed':
      return { ...state, remapRules: e.rules };
  }
}

type Listener = () => void;
type SwitchListener = (controlledDeviceId: string) => void;

/**
 * The one object the UI talks to. Holds the engine-derived state (a tiny
 * external store consumed with `useSyncExternalStore`) and exposes one typed
 * method per contract command.
 */
/**
 * Where input is going, from this computer's point of view: a controller's
 * active screen, or on a controlled computer `"local"` while the controller
 * drives it and `"away"` otherwise.
 */
function focusOf(c: ControlState | null | undefined): string | undefined {
  if (!c) return undefined;
  return c.role === 'controlled' ? (c.entered ? 'local' : 'away') : c.activeScreen;
}

export class FlowClient {
  private state: FlowState = initialState;
  private seen = new Set<FlowEvent['type']>();
  private listeners = new Set<Listener>();
  private activeScreenListeners = new Set<SwitchListener>();

  constructor(readonly transport: Transport) {}

  /** True when a desktop shell hosts this UI (the tray popover is a separate window). */
  get hasDesktopShell(): boolean {
    return this.transport.kind === 'tauri';
  }

  connect(): void {
    this.transport.connect({
      onEvent: (wire) => {
        const e = decodeEvent(wire);
        if (e) this.apply(e);
      },
      onStatus: (status, error) => this.onStatus(status, error),
    });
  }

  close(): void {
    this.transport.close();
  }

  // --- store ---------------------------------------------------------------
  getSnapshot = (): FlowState => this.state;

  subscribe = (l: Listener): (() => void) => {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  };

  /** Fires whenever `control_state.active_screen` changes (drives the switch HUD). */
  onActiveScreenChanged(l: SwitchListener): () => void {
    this.activeScreenListeners.add(l);
    return () => this.activeScreenListeners.delete(l);
  }

  private set(next: FlowState): void {
    this.state = next;
    this.listeners.forEach((l) => l());
  }

  private apply(e: FlowEvent): void {
    const prevFocus = focusOf(this.state.controlState);
    this.seen.add(e.type);
    let next = reduce(this.state, e);
    const loaded = SNAPSHOT.every((t) => this.seen.has(t));
    if (loaded !== next.loaded) next = { ...next, loaded };
    this.set(next);
    if (e.type === 'control_state_changed') {
      const focus = focusOf(e.controlState);
      if (prevFocus !== undefined && prevFocus !== focus)
        this.activeScreenListeners.forEach((l) => l(focus!));
    }
  }

  private onStatus(status: TransportStatus, error?: string): void {
    this.set({
      ...this.state,
      connection: status,
      connectionError: status === 'closed' ? (error ?? null) : null,
    });
  }

  // --- commands (names/payloads = contract 0.2.0) --------------------------
  private cmd<T = void>(command: string, payload?: unknown): Promise<T> {
    return this.transport.send(command, payload === undefined ? {} : toWire(payload)) as Promise<T>;
  }

  /** `target`: `"local"` (return home) or a DeviceId. */
  switchTo = (target: string) => this.cmd('switch_to', { target });
  releaseControl = () => this.cmd('release_control');
  removeDevice = (deviceId: string) => this.cmd('remove_device', { deviceId });
  startPairing = () => this.cmd('start_pairing');
  cancelPairing = () => this.cmd('cancel_pairing');
  pairWithCandidate = (candidateId: string) => this.cmd('pair_with_candidate', { candidateId });
  respondToPairingRequest = (requestId: string, decision: 'accept' | 'reject') =>
    this.cmd('respond_to_pairing_request', { requestId, decision });
  setSwitchKey = (binding: SwitchKeyBinding) => this.cmd('set_switch_key', binding);
  updateSettings = (patch: SettingsPatch) => this.cmd('update_settings', patch);
  resetSettings = () => this.cmd('reset_settings');
  requestPermission = () => this.cmd('request_permission');
  retryConnection = () => this.cmd('retry_connection');
  /** "Check Again": the engine re-probes the OS permissions now. */
  recheckPermissions = () => this.cmd('recheck_permissions');
  /** Stops automatic redialling after a lost link, until `retryConnection`. */
  cancelReconnect = () => this.cmd('cancel_reconnect');
  /**
   * Opens the OS settings page where `name` (a permission item) is granted.
   * The desktop shell opens it itself; any other client gets the URL and
   * opens it here.
   */
  openPermissionSettings = async (name: string): Promise<void> => {
    const r = await this.cmd<{ url?: string | null }>('open_permission_settings', { name });
    if (r?.url && this.transport.kind !== 'tauri') window.open(r.url, '_blank', 'noopener');
  };
  /** Tray popover → full window, optionally with a sheet open. Desktop shell only. */
  openMainWindow = (target: { route?: string; sheet?: 'pairing' | 'permission' } = {}) =>
    this.cmd('open_main_window', target);
  setScreenLayout = (layout: ScreenLayout) => this.cmd('set_screen_layout', { layout });
  setRole = (role: Role) => this.cmd('set_role', { role });
  getKeymap = (os: HostOs) => this.cmd<{ os: HostOs; entries: KeymapEntry[] }>('get_keymap', { os });
  setRemapRules = (rules: RemapRule[]) => this.cmd('set_remap_rules', { rules });
  revealConfigDir = () => this.cmd('reveal_config_dir');
  /** Resolves with the saved file path, when the engine reports one. */
  exportDiagnostics = () => this.cmd<{ path?: string } | undefined>('export_diagnostics');
}
