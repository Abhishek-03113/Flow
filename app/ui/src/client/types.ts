/**
 * Domain models mirroring `docs/contracts/data-model.md` (+ contract 0.2.0 in
 * `docs/redesign/PLAN.md`). Object keys are camelCase here and snake_case on
 * the wire (see `mapper.ts`). String-literal enum VALUES keep their wire
 * spelling (`permission_required`, `role_conflict`) so there is no value mapper.
 */

export type HostOs = 'macos' | 'windows' | 'linux';
export type Role = 'controller' | 'controlled';

export type DeviceState = 'pairing' | 'connected' | 'active' | 'inactive' | 'disconnected' | 'error';

/** Why the last link attempt to a paired device failed (`Device.link_error`, `flow_core::link::link_error`). */
export const LINK_ERROR_CODES = [
  'role_conflict',
  'protocol_mismatch',
  'protocol_error',
  'hello_timeout',
  'connection_lost',
  'injector_unavailable',
] as const;
export type LinkErrorCode = (typeof LINK_ERROR_CODES)[number];

export interface Device {
  id: string;
  name: string;
  os: HostOs;
  state: DeviceState;
  role: Role | null;
  screen: { width: number; height: number } | null;
  /** Unix ms. Derived from the 0.1.x `last_seen` ISO string when `last_seen_ms` is absent. */
  lastSeenMs: number;
  linkError: LinkErrorCode | null;
  /** True for "this computer"; sent by the engine. */
  isLocal: boolean;
}

export type LinkState =
  | 'connected'
  | 'connecting'
  | 'reconnecting'
  | 'disconnected'
  | 'error'
  | 'permission_required';

export type PairingStage = 'idle' | 'searching' | 'found' | 'requesting' | 'paired' | 'failed';

export interface PairingCandidate {
  id: string;
  name: string;
  os: HostOs;
}

export interface PairingSession {
  stage: PairingStage;
  candidates: PairingCandidate[];
  targetName: string | null;
  error: string | null;
  /** Short verification code shown on both computers while `requesting` (optional, G-25). */
  code: string | null;
}

export interface IncomingPairingRequest {
  requestId: string;
  deviceName: string;
  deviceOs: HostOs;
  fingerprint: string;
  address: string;
}

export interface SwitchKeyBinding {
  label: string;
  /** Platform-neutral tokens: `Ctrl Alt Shift Meta ScrollLock Pause F13 Space A …`. */
  keys: string[];
}

export type PointerSensitivity = 'low' | 'normal' | 'high';

export interface FlowSettings {
  launchAtLogin: boolean;
  showTrayIcon: boolean;
  autoReconnect: boolean;
  autoConnectPairedDevices: boolean;
  shareKeyboard: boolean;
  shareMouse: boolean;
  debugLogging: boolean;
  pointerSensitivity: PointerSensitivity;
  switchKey: SwitchKeyBinding;
  role: Role;
  returnHomeKey: SwitchKeyBinding | null;
  edgeSwitching: boolean;
  edgeSwitchDelayMs: number;
  /** Read-only: where the user's keymap/remap files live. */
  keymapOverridesPath: string;
}

export type SettingsPatch = Partial<Omit<FlowSettings, 'keymapOverridesPath'>>;

export interface FixStep {
  text: string;
  /** A shell command to copy, when the step is one. */
  command: string | null;
}

/** One plain sentence, then ordered steps. */
export interface HowToFix {
  summary: string;
  steps: FixStep[];
}

/** One thing the OS must allow (engine-supplied; macOS lists two, Linux two, Windows none). */
export interface PermissionItem {
  name: string;
  granted: boolean;
  /** `null` while granted. */
  howToFix: HowToFix | null;
}

export interface PermissionStatus {
  /** Summary label. */
  name: string;
  /** True when every item is granted. */
  granted: boolean;
  items: PermissionItem[];
}

export type ActiveScreen = 'local' | string;

export interface ControlState {
  role: Role;
  /**
   * Controller: `"local"` or the DeviceId of the screen receiving input.
   * Controlled: always `"local"` (this computer is the only screen it has);
   * whether the controller is driving it right now is `entered`.
   */
  activeScreen: ActiveScreen;
  /** Controlled only: the linked controller, if any. */
  controllerDeviceId: string | null;
  /** Controlled only: true while the controller is driving THIS computer. */
  entered: boolean;
}

export interface ScreenRect {
  deviceId: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface ScreenLayout {
  screens: ScreenRect[];
}

export interface Diagnostics {
  /** `null` until the first switch happened. */
  switchLatencyMsP50: number | null;
  switchLatencyMsP95: number | null;
  eventsForwarded: number;
  eventsDropped: number;
  uptimeS: number;
}

export interface RemapRule {
  from: string;
  to: string;
  whenTargetOs: HostOs | null;
}

/** One key of a keymap file: the wire name and that OS's native key code. */
export interface KeymapKey {
  name: string;
  code: number;
}

/** `get_keymap`'s answer: the effective keymap file (`config/keymaps/<os>.toml` + user overrides). */
export interface Keymap {
  version: number;
  os: HostOs;
  keys: KeymapKey[];
}

export type ConnectionStatus = 'connecting' | 'open' | 'closed';

/** Everything the UI renders from. Built purely from engine pushes. */
export interface FlowState {
  connection: ConnectionStatus;
  /** Last transport-level failure (e.g. the WebSocket could not connect). */
  connectionError: string | null;
  /** True once every snapshot event has arrived at least once. */
  loaded: boolean;
  devices: Device[];
  linkState: LinkState;
  pairingSession: PairingSession;
  settings: FlowSettings | null;
  permission: PermissionStatus | null;
  incomingPairingRequest: IncomingPairingRequest | null;
  controlState: ControlState | null;
  screenLayout: ScreenLayout;
  diagnostics: Diagnostics | null;
  remapRules: RemapRule[];
}
