import { describe, expect, it } from 'vitest';
import { LINK_ERROR_CODES, type Device } from './client/types';
import { formatMs, linkErrorCopy } from './copy';

describe('formatMs', () => {
  it('says what was measured, never a misleading 0.0', () => {
    expect(formatMs(null)).toBe('—');
    expect(formatMs(0)).toBe('<1 ms');
    expect(formatMs(0.04)).toBe('<1 ms');
    expect(formatMs(2.345)).toBe('2.3 ms');
    expect(formatMs(11.4)).toBe('11 ms');
  });
});

describe('linkErrorCopy', () => {
  const pc: Device = {
    id: 'd2',
    name: 'Office PC',
    os: 'windows',
    state: 'error',
    role: 'controller',
    screen: null,
    lastSeenMs: 0,
    linkError: null,
    isLocal: false,
  };

  it('has a title and a detail naming the computer for every engine code', () => {
    for (const code of LINK_ERROR_CODES) {
      const copy = linkErrorCopy(code, pc, 'controller');
      expect(copy.title, code).toBeTruthy();
      expect(copy.detail, code).toContain('Office PC');
    }
  });
});

describe('friendlyError', () => {
  it('has copy for every engine error code', async () => {
    const { readFileSync } = await import('node:fs');
    const { resolve } = await import('node:path');
    const { ERRORS } = await import('./copy');
    const rust = readFileSync(resolve(process.cwd(), '../../core/src/error/mod.rs'), 'utf8');
    const codes = [...rust.matchAll(/=> "([a-z_]+)",/g)]
      .map((m) => m[1] as string)
      .filter((c) => c !== 'internal_error');
    expect(codes).toContain('no_controller');
    for (const code of codes) expect(ERRORS[code], code).toBeTruthy();
  });
});
