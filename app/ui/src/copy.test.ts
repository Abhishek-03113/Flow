import { describe, expect, it } from 'vitest';
import { formatMs } from './copy';

describe('formatMs', () => {
  it('says what was measured, never a misleading 0.0', () => {
    expect(formatMs(null)).toBe('—');
    expect(formatMs(0)).toBe('<1 ms');
    expect(formatMs(0.04)).toBe('<1 ms');
    expect(formatMs(2.345)).toBe('2.3 ms');
    expect(formatMs(11.4)).toBe('11 ms');
  });
});
