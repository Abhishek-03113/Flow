import { describe, expect, it } from 'vitest';
import { decodeEvent } from './events';

describe('decodeEvent', () => {
  it('normalises 0.1.x devices (ISO last_seen, no role) and marks d1 as local', () => {
    const e = decodeEvent({
      event: 'devices_changed',
      payload: [
        { id: 'd1', name: 'MacBook', os: 'macos', state: 'active', last_seen: '2026-08-25T06:58:00Z' },
      ],
    });
    expect(e?.type).toBe('devices_changed');
    if (e?.type !== 'devices_changed') throw new Error('unreachable');
    expect(e.devices[0]).toMatchObject({
      isLocal: true,
      role: null,
      linkError: null,
      lastSeenMs: Date.parse('2026-08-25T06:58:00Z'),
    });
  });

  it('decodes 0.2.0 devices with link_error', () => {
    const e = decodeEvent({
      event: 'devices_changed',
      payload: [
        {
          id: 'd2',
          name: 'PC',
          os: 'windows',
          state: 'error',
          role: 'controller',
          last_seen_ms: 42,
          link_error: 'role_conflict',
          screen: { width: 1, height: 2 },
        },
      ],
    });
    if (e?.type !== 'devices_changed') throw new Error('unreachable');
    expect(e.devices[0]).toMatchObject({
      isLocal: false,
      role: 'controller',
      linkError: 'role_conflict',
      lastSeenMs: 42,
    });
  });

  it('decodes control state, layout and an empty incoming request', () => {
    const c = decodeEvent({
      event: 'control_state_changed',
      payload: { role: 'controller', active_screen: 'd2', controller_device_id: null },
    });
    expect(c).toEqual({
      type: 'control_state_changed',
      controlState: { role: 'controller', activeScreen: 'd2', controllerDeviceId: null },
    });
    const l = decodeEvent({
      event: 'screen_layout_changed',
      payload: { screens: [{ device_id: 'd1', x: 0, y: 0, width: 10, height: 5 }] },
    });
    expect(l).toMatchObject({ layout: { screens: [{ deviceId: 'd1' }] } });
    expect(decodeEvent({ event: 'incoming_pairing_request_changed', payload: null })).toEqual({
      type: 'incoming_pairing_request_changed',
      request: null,
    });
  });

  it('ignores unknown events (forward compatible)', () => {
    expect(decodeEvent({ event: 'from_the_future', payload: {} })).toBeNull();
  });
});
