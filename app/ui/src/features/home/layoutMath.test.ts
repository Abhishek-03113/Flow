import { describe, expect, it } from 'vitest';
import { adjacency, nextSlot, overlaps, snap } from './layoutMath';

const A = { x: 0, y: 0, width: 100, height: 60 };

describe('layoutMath', () => {
  it('snaps a dropped tile flush against the nearest edge', () => {
    const dropped = { x: 112, y: 9, width: 80, height: 50 }; // near A's right edge, a bit low
    const s = snap(dropped, [A]);
    expect(s.x).toBe(100);
    expect([0, 5, 10]).toContain(s.y); // top, centre or bottom aligned: 0 is nearest to 9? centre=5
    expect(overlaps(s, A)).toBe(false);
  });

  it('never returns an overlapping position', () => {
    const s = snap({ x: 40, y: 20, width: 80, height: 50 }, [A]);
    expect(overlaps(s, A)).toBe(false);
  });

  it('prefers top/bottom attach when dropped above or below', () => {
    expect(snap({ x: 10, y: 75, width: 80, height: 50 }, [A]).y).toBe(60);
    expect(snap({ x: 10, y: -70, width: 80, height: 50 }, [A]).y).toBe(-50);
  });

  it('keyboard nextSlot moves in the requested direction only', () => {
    const m = { x: 100, y: 0, width: 80, height: 50 };
    const left = nextSlot(m, [A], 'left');
    expect(left!.x).toBeLessThan(100);
    expect(nextSlot(m, [A], 'right')).toBeNull();
    expect(nextSlot(m, [A], 'down')?.y).toBe(60);
  });

  it('derives adjacency from touching edges', () => {
    expect(
      adjacency({
        screens: [
          { deviceId: 'a', ...A },
          { deviceId: 'b', x: 100, y: 10, width: 80, height: 50 },
          { deviceId: 'c', x: 300, y: 0, width: 50, height: 50 },
        ],
      }),
    ).toEqual([['a', 'b']]);
  });
});
