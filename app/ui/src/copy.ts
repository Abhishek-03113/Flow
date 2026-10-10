import { isFlowCommandError } from './client/errors';
import type { Device, HostOs, LinkErrorCode, Role } from './client/types';

/** Friendly copy for stable engine error codes. Raw `message` strings never reach the user. */
export const ERRORS: Record<string, string> = {
  device_not_found: "That computer isn't available any more.",
  device_not_switchable: "Flow can't reach that computer right now.",
  device_not_removable: "This computer can't be removed.",
  pairing_in_progress: 'Flow is already looking for computers.',
  pairing_not_active: 'There is nothing to cancel.',
  pairing_not_ready: "Flow isn't ready to connect yet. Try again in a moment.",
  candidate_not_found: "That computer isn't nearby any more.",
  invalid_switch_key: 'Choose at least one key for the shortcut.',
  permission_already_granted: 'Flow already has permission.',
  link_not_recoverable: 'Flow is already connected.',
  pairing_request_not_found: 'That request has expired.',
  not_controller: 'Only the computer that shares its keyboard and mouse can switch screens.',
  no_controller: 'No computer is sharing its keyboard and mouse yet.',
  permission_not_found: "Flow doesn't need that permission.",
  not_reconnecting: 'That computer is not trying to reconnect.',
  not_controlled: 'Nothing is controlling this computer right now.',
  invalid_layout: "Screens can't overlap. Try placing it next to another screen.",
  invalid_remap_rule: 'A key can only become another key, and a modifier another modifier.',
  invalid_payload: "That value isn't allowed.",
  connection_closed: "Flow isn't responding.",
  engine_not_embedded: 'Flow is still starting up.',
  unknown_command: "This version of Flow can't do that yet.",
};

export function friendlyError(e: unknown): string {
  if (isFlowCommandError(e)) return ERRORS[e.code] ?? 'Something went wrong. Please try again.';
  return 'Something went wrong. Please try again.';
}

export const osName = (os: HostOs): string => ({ macos: 'macOS', windows: 'Windows', linux: 'Linux' })[os];

export const roleLabel = (r: Role | null): string =>
  r === 'controller' ? 'Shares keyboard & mouse' : r === 'controlled' ? 'Controlled' : 'Unknown role';

export const roleShort = (r: Role): string => (r === 'controller' ? 'Controller' : 'Controlled');

/** Actionable text for a link error. `action` names the UI affordance that fixes it. */
export function linkErrorCopy(
  code: LinkErrorCode,
  device: Device,
  localRole: Role,
): { title: string; detail: string; action?: 'role' } {
  switch (code) {
    case 'role_conflict':
      return {
        title: 'Both computers are set up the same way',
        detail:
          localRole === 'controller'
            ? `${device.name} also shares its keyboard and mouse. On one of them, choose "Control this computer from another one" in General.`
            : `${device.name} is also set to be controlled. One of you needs to share the keyboard and mouse.`,
        action: 'role',
      };
    case 'protocol_mismatch':
      return {
        title: 'Different Flow versions',
        detail: `Update Flow on ${device.name} so both computers run the same version.`,
      };
    case 'protocol_error':
      return {
        title: "Couldn't connect",
        detail: `${device.name} answered in a way Flow didn't expect. Update Flow on both computers, then try again.`,
      };
    case 'hello_timeout':
    case 'connection_lost':
      return {
        title: "Can't reach it",
        detail: `Make sure ${device.name} is on, awake and on the same network. Flow keeps trying.`,
      };
    case 'injector_unavailable':
      return {
        title: "Flow can't use the keyboard and mouse here",
        detail: `Check that Flow has permission to control this computer, then try again from ${device.name}.`,
      };
    default:
      return {
        title: "Couldn't connect",
        detail: `Something went wrong connecting to ${device.name}. Flow keeps trying.`,
      };
  }
}

export function formatRelative(ms: number, now = Date.now()): string {
  const s = Math.round((ms - now) / 1000);
  if (Math.abs(s) < 45) return 'just now';
  const rtf = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });
  const m = Math.round(s / 60);
  if (Math.abs(m) < 60) return rtf.format(m, 'minute');
  const h = Math.round(m / 60);
  if (Math.abs(h) < 24) return rtf.format(h, 'hour');
  return rtf.format(Math.round(h / 24), 'day');
}

export function formatUptime(s: number): string {
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h} h ${m} min` : m > 0 ? `${m} min` : `${s} s`;
}

export const trayName = (os: HostOs): string => (os === 'macos' ? 'menu bar' : 'system tray');

/** A latency in milliseconds, or a dash while nothing has been measured. */
export function formatMs(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) return '—';
  // The engine measures the switch itself, which on a fast machine is well under 1 ms.
  if (ms < 1) return '<1 ms';
  return `${ms < 10 ? ms.toFixed(1) : Math.round(ms)} ms`;
}
