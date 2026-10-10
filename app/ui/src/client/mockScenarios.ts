import { LOCAL_DEVICE_ID } from './events';
import type {
  ControlState,
  Diagnostics,
  FlowSettings,
  HostOs,
  IncomingPairingRequest,
  LinkState,
  PairingSession,
  PermissionStatus,
  RemapRule,
  ScreenLayout,
  Device,
} from './types';

/** Devices as the engine sends them (no client-derived `isLocal`). */
export type EngineDevice = Omit<Device, 'isLocal'>;

export interface EngineState {
  devices: EngineDevice[];
  linkState: LinkState;
  pairingSession: PairingSession;
  settings: FlowSettings;
  permission: PermissionStatus;
  incomingPairingRequest: IncomingPairingRequest | null;
  controlState: ControlState;
  screenLayout: ScreenLayout;
  diagnostics: Diagnostics;
  remapRules: RemapRule[];
}

export const SCENARIOS = [
  'fresh-install',
  'two-devices',
  'three-devices',
  'controlling-remote',
  'controlled',
  'role-conflict',
  'permission-missing',
  'connecting',
  'reconnecting',
  'disconnected',
  'error',
  'pairing-request',
  'no-devices',
] as const;
export type ScenarioName = (typeof SCENARIOS)[number];

export const defaultSettings = (): FlowSettings => ({
  launchAtLogin: false,
  showTrayIcon: true,
  autoReconnect: true,
  autoConnectPairedDevices: true,
  shareKeyboard: true,
  shareMouse: true,
  debugLogging: false,
  pointerSensitivity: 'normal',
  switchKey: { label: 'Scroll Lock', keys: ['ScrollLock'] },
  role: 'controller',
  returnHomeKey: null,
  edgeSwitching: true,
  edgeSwitchDelayMs: 0,
  keymapOverridesPath: '~/.config/flow-daemon/keymaps',
});

const NOW = Date.now();
const MIN = 60_000;

const permissionFor = (os: HostOs, granted: boolean): PermissionStatus => ({
  name:
    os === 'macos' ? 'Accessibility access' : os === 'windows' ? 'Input monitoring' : 'Input device access',
  granted,
});

const local = (os: HostOs, role: 'controller' | 'controlled', active: boolean): EngineDevice => ({
  id: LOCAL_DEVICE_ID,
  name: os === 'macos' ? 'MacBook Pro' : os === 'windows' ? 'Desk PC' : 'Workstation',
  os,
  state: active ? 'active' : 'inactive',
  role,
  screen: { width: 1512, height: 982 },
  lastSeenMs: NOW,
  linkError: null,
});

const officePc: EngineDevice = {
  id: 'd2',
  name: 'Office PC',
  os: 'windows',
  state: 'inactive',
  role: 'controlled',
  screen: { width: 1920, height: 1080 },
  lastSeenMs: NOW - 2 * MIN,
  linkError: null,
};
const studioMini: EngineDevice = {
  id: 'd3',
  name: 'Studio Mac mini',
  os: 'macos',
  state: 'inactive',
  role: 'controlled',
  screen: { width: 2560, height: 1440 },
  lastSeenMs: NOW - 14 * MIN,
  linkError: null,
};

export function layoutFor(devices: EngineDevice[]): ScreenLayout {
  let x = 0;
  const screens = devices
    .filter((d) => d.screen)
    .map((d) => {
      const s = { deviceId: d.id, x, y: 0, width: d.screen!.width, height: d.screen!.height };
      x += s.width;
      return s;
    });
  return { screens };
}

const diagnostics = (): Diagnostics => ({
  switchLatencyMsP50: 11,
  switchLatencyMsP95: 24,
  eventsForwarded: 18_204,
  eventsDropped: 3,
  uptimeS: 5_412,
});

const base = (os: HostOs, devices: EngineDevice[]): EngineState => ({
  devices,
  linkState: 'connected',
  pairingSession: { stage: 'idle', candidates: [], targetName: null, error: null, code: null },
  settings: defaultSettings(),
  permission: permissionFor(os, true),
  incomingPairingRequest: null,
  controlState: { role: 'controller', activeScreen: 'local', controllerDeviceId: null },
  screenLayout: layoutFor(devices),
  diagnostics: diagnostics(),
  remapRules: [{ from: 'Meta', to: 'Ctrl', whenTargetOs: 'windows' }],
});

export const scenarioHasCompletedOnboarding = (name: string): boolean => name !== 'fresh-install';

/** Seeds the mock engine. `os` is the OS of "this computer". */
export function seedScenario(name: string, os: HostOs): EngineState {
  const me = (active = true) => local(os, 'controller', active);
  switch (name) {
    case 'fresh-install': {
      const s = base(os, [me()]);
      s.permission = permissionFor(os, os !== 'macos');
      s.linkState = s.permission.granted ? 'connected' : 'permission_required';
      s.remapRules = [];
      s.diagnostics = { ...diagnostics(), eventsForwarded: 0, eventsDropped: 0, uptimeS: 12 };
      return s;
    }
    case 'no-devices': {
      const s = base(os, [me()]);
      s.remapRules = [];
      return s;
    }
    case 'three-devices': {
      const s = base(os, [me(), officePc, studioMini]);
      return s;
    }
    case 'controlling-remote': {
      const s = base(os, [me(false), { ...officePc, state: 'active', lastSeenMs: NOW }, studioMini]);
      s.controlState = { role: 'controller', activeScreen: 'd2', controllerDeviceId: null };
      return s;
    }
    case 'controlled': {
      const controller: EngineDevice = {
        ...officePc,
        name: 'Desk PC',
        role: 'controller',
        state: 'active',
        lastSeenMs: NOW,
      };
      const myself = { ...local(os, 'controlled', true) };
      const s = base(os, [myself, controller]);
      s.settings.role = 'controlled';
      s.controlState = { role: 'controlled', activeScreen: 'local', controllerDeviceId: 'd2' };
      return s;
    }
    case 'role-conflict': {
      const conflicted: EngineDevice = {
        ...officePc,
        role: 'controller',
        state: 'error',
        linkError: 'role_conflict',
      };
      const s = base(os, [me(), conflicted]);
      s.linkState = 'error';
      return s;
    }
    case 'permission-missing': {
      const s = base(os, [me(), officePc]);
      s.permission = permissionFor(os, false);
      s.linkState = 'permission_required';
      return s;
    }
    case 'connecting': {
      const s = base(os, [me(), { ...officePc, state: 'disconnected' }]);
      s.linkState = 'connecting';
      return s;
    }
    case 'reconnecting': {
      const s = base(os, [me(), { ...officePc, state: 'disconnected' }]);
      s.linkState = 'reconnecting';
      return s;
    }
    case 'disconnected': {
      const s = base(os, [me(), { ...officePc, state: 'disconnected', lastSeenMs: NOW - 47 * MIN }]);
      s.linkState = 'disconnected';
      return s;
    }
    case 'error': {
      const s = base(os, [me(), { ...officePc, state: 'error', linkError: 'unreachable' }]);
      s.linkState = 'error';
      return s;
    }
    case 'pairing-request':
      return base(os, [me(), officePc]);
    case 'two-devices':
    default:
      return base(os, [me(), officePc]);
  }
}

export const sampleIncomingRequest: IncomingPairingRequest = {
  requestId: 'ipr-9f2c1a',
  deviceName: "Alex's Windows laptop",
  deviceOs: 'windows',
  fingerprint: '3f2a 91c4 8d10 6b57',
  address: '192.168.0.103',
};

export const candidatePool = [
  { id: 'cand-1', name: 'Studio Mac mini', os: 'macos' as const },
  { id: 'cand-2', name: 'Living Room PC', os: 'windows' as const },
  { id: 'cand-3', name: 'Dev Laptop', os: 'linux' as const },
];
