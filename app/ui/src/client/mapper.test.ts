import { describe, expect, it } from 'vitest';
import { fromWire, toWire } from './mapper';

describe('mapper', () => {
  it('maps snake_case keys to camelCase recursively, leaving values alone', () => {
    const out = fromWire<{
      switchKey: { keys: string[] };
      devices: { lastSeenMs: number; linkError: string }[];
    }>({
      switch_key: { keys: ['Ctrl', 'Shift'] },
      devices: [{ last_seen_ms: 5, link_error: 'role_conflict' }],
    });
    expect(out.switchKey.keys).toEqual(['Ctrl', 'Shift']);
    expect(out.devices[0]).toEqual({ lastSeenMs: 5, linkError: 'role_conflict' });
  });

  it('round-trips camelCase to snake_case and back', () => {
    const domain = { edgeSwitchDelayMs: 120, rules: [{ whenTargetOs: 'windows' }], nothing: null };
    expect(toWire(domain)).toEqual({
      edge_switch_delay_ms: 120,
      rules: [{ when_target_os: 'windows' }],
      nothing: null,
    });
    expect(fromWire(toWire(domain))).toEqual(domain);
  });

  it('passes primitives and null through', () => {
    expect(fromWire('permission_required')).toBe('permission_required');
    expect(fromWire(null)).toBeNull();
  });
});
