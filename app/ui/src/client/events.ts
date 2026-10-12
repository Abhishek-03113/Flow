import { fromWire } from './mapper';
import { LINK_ERROR_CODES } from './types';
import type {
  ControlState,
  Device,
  Diagnostics,
  FlowSettings,
  IncomingPairingRequest,
  LinkErrorCode,
  LinkState,
  PairingSession,
  PermissionStatus,
  RemapRule,
  ScreenLayout,
} from './types';
import type { WireEvent } from './transport';

/** Typed engine pushes (contract 0.1.3 + 0.2.0). */
export type FlowEvent =
  | { type: 'devices_changed'; devices: Device[] }
  | { type: 'link_state_changed'; linkState: LinkState }
  | { type: 'pairing_session_changed'; session: PairingSession }
  | { type: 'settings_changed'; settings: FlowSettings }
  | { type: 'permission_changed'; permission: PermissionStatus }
  | { type: 'incoming_pairing_request_changed'; request: IncomingPairingRequest | null }
  | { type: 'control_state_changed'; controlState: ControlState }
  | { type: 'screen_layout_changed'; layout: ScreenLayout }
  | { type: 'diagnostics_changed'; diagnostics: Diagnostics }
  | { type: 'remap_rules_changed'; rules: RemapRule[] };

type RawDevice = Omit<Partial<Device>, 'lastSeenMs' | 'linkError'> & {
  id: string;
  lastSeen?: string;
  lastSeenMs?: number;
  linkError?: string | null;
};

/** A code this UI has no copy for (a newer engine) still means the link failed. */
function toLinkError(code: string | null | undefined): LinkErrorCode | null {
  if (!code) return null;
  return (LINK_ERROR_CODES as readonly string[]).includes(code) ? (code as LinkErrorCode) : 'protocol_error';
}

/** Normalises 0.1.x devices (ISO `last_seen`, no role) and 0.2.0 devices alike. */
function toDevice(raw: RawDevice): Device {
  const lastSeenMs = raw.lastSeenMs ?? (raw.lastSeen ? Date.parse(raw.lastSeen) : 0);
  return {
    id: raw.id,
    name: raw.name ?? 'Unnamed computer',
    os: raw.os ?? 'linux',
    state: raw.state ?? 'disconnected',
    role: raw.role ?? null,
    screen: raw.screen ?? null,
    lastSeenMs: Number.isFinite(lastSeenMs) ? lastSeenMs : 0,
    linkError: toLinkError(raw.linkError),
    isLocal: raw.isLocal === true,
  };
}

function normalizePermission(p: Partial<PermissionStatus>): PermissionStatus {
  return {
    name: p.name ?? 'Input access',
    granted: p.granted ?? false,
    items: (p.items ?? []).map((i) => ({
      name: i.name,
      granted: i.granted,
      howToFix: i.howToFix
        ? {
            summary: i.howToFix.summary,
            steps: i.howToFix.steps.map((s) => ({ text: s.text, command: s.command ?? null })),
          }
        : null,
    })),
    oneClickSetup: p.oneClickSetup === true,
    restartRequired: p.restartRequired === true,
  };
}

/** Returns null for events this UI does not know (forward compatible). */
export function decodeEvent(wire: WireEvent): FlowEvent | null {
  const p = fromWire<never>(wire.payload) as unknown;
  switch (wire.event) {
    case 'devices_changed':
      return { type: wire.event, devices: (p as RawDevice[]).map(toDevice) };
    case 'link_state_changed':
      return { type: wire.event, linkState: p as LinkState };
    case 'pairing_session_changed': {
      const s = p as Partial<PairingSession>;
      return {
        type: wire.event,
        session: {
          stage: s.stage ?? 'idle',
          candidates: s.candidates ?? [],
          targetName: s.targetName ?? null,
          error: s.error ?? null,
          code: s.code ?? null,
        },
      };
    }
    case 'settings_changed':
      return { type: wire.event, settings: p as FlowSettings };
    case 'permission_changed':
      return { type: wire.event, permission: normalizePermission(p as Partial<PermissionStatus>) };
    case 'incoming_pairing_request_changed':
      return { type: wire.event, request: (p as IncomingPairingRequest | null) ?? null };
    case 'control_state_changed':
      return { type: wire.event, controlState: p as ControlState };
    case 'screen_layout_changed':
      return { type: wire.event, layout: p as ScreenLayout };
    case 'diagnostics_changed':
      return { type: wire.event, diagnostics: p as Diagnostics };
    case 'remap_rules_changed':
      return { type: wire.event, rules: p as RemapRule[] };
    default:
      return null;
  }
}
