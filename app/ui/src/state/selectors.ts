import type { Device, FlowState, HostOs, Role } from '../client/types';

export const localDevice = (s: FlowState): Device | undefined => s.devices.find((d) => d.isLocal);
export const remoteDevices = (s: FlowState): Device[] => s.devices.filter((d) => !d.isLocal);

/** OS of this computer; falls back to the browser's guess before the snapshot arrives. */
export function localOs(s: FlowState): HostOs {
  const d = localDevice(s);
  if (d) return d.os;
  const ua = typeof navigator === 'undefined' ? '' : navigator.userAgent;
  return /Mac/i.test(ua) ? 'macos' : /Win/i.test(ua) ? 'windows' : 'linux';
}

/**
 * What the link status should say. The engine reports `disconnected` when
 * there is nothing to be connected to; with no paired computer that is not
 * an error, so it becomes the neutral `no_computers`.
 */
export type EffectiveLink = FlowState['linkState'] | 'no_computers';
export const effectiveLink = (s: FlowState): EffectiveLink =>
  s.linkState === 'disconnected' && remoteDevices(s).length === 0 ? 'no_computers' : s.linkState;

export const myRole = (s: FlowState): Role => s.controlState?.role ?? s.settings?.role ?? 'controller';

export const isReachable = (d: Device): boolean =>
  !d.linkError && (d.state === 'inactive' || d.state === 'connected' || d.state === 'active');

/** Devices a controller can switch to right now. */
export const switchTargets = (s: FlowState): Device[] => remoteDevices(s).filter(isReachable);

export const deviceById = (s: FlowState, id: string): Device | undefined =>
  s.devices.find((d) => d.id === id);

export type Situation =
  | { kind: 'controller-home' }
  | { kind: 'controller-remote'; target: Device | undefined }
  | { kind: 'controlled-driven'; controller: Device | undefined }
  | { kind: 'controlled-waiting'; controller: Device | undefined }
  | { kind: 'controlled-unlinked' };

/** Who controls what, derived from `control_state` (see `ControlState` docs). */
export function situation(s: FlowState): Situation {
  const c = s.controlState;
  if (!c || c.role === 'controller') {
    if (!c || c.activeScreen === 'local') return { kind: 'controller-home' };
    return { kind: 'controller-remote', target: deviceById(s, c.activeScreen) };
  }
  const controller = c.controllerDeviceId ? deviceById(s, c.controllerDeviceId) : undefined;
  if (!c.controllerDeviceId) return { kind: 'controlled-unlinked' };
  return c.entered ? { kind: 'controlled-driven', controller } : { kind: 'controlled-waiting', controller };
}
