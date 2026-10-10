import type { ScreenLayout, ScreenRect } from '../../client/types';

export type Rect = Pick<ScreenRect, 'x' | 'y' | 'width' | 'height'>;
export type Dir = 'left' | 'right' | 'up' | 'down';

export const overlaps = (a: Rect, b: Rect): boolean =>
  a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

/** Every position where `m` touches an edge of some other screen (aligned top/centre/bottom or left/centre/right). */
export function attachPositions(m: Rect, others: Rect[]): Rect[] {
  const out: Rect[] = [];
  for (const o of others) {
    const ys = [o.y, o.y + (o.height - m.height) / 2, o.y + o.height - m.height];
    const xs = [o.x, o.x + (o.width - m.width) / 2, o.x + o.width - m.width];
    for (const y of ys) {
      out.push({ ...m, x: o.x + o.width, y }, { ...m, x: o.x - m.width, y });
    }
    for (const x of xs) {
      out.push({ ...m, x, y: o.y + o.height }, { ...m, x, y: o.y - m.height });
    }
  }
  return out
    .map((r) => ({ ...r, x: Math.round(r.x), y: Math.round(r.y) }))
    .filter((r) => !others.some((o) => overlaps(r, o)));
}

const dist = (a: Rect, b: Rect) => Math.hypot(a.x - b.x, a.y - b.y);

/** Snaps a freely dragged rect to the nearest edge-touching, non-overlapping position. */
export function snap(m: Rect, others: Rect[]): Rect {
  if (others.length === 0) return m;
  const cands = attachPositions(m, others);
  if (cands.length === 0) return m;
  return cands.reduce((best, c) => (dist(c, m) < dist(best, m) ? c : best));
}

/** Keyboard move: the nearest valid attach position strictly in direction `dir`. */
export function nextSlot(m: Rect, others: Rect[], dir: Dir): Rect | null {
  // Require a meaningful move (half a tile) along the requested axis.
  const inDir = (c: Rect) =>
    dir === 'left'
      ? c.x < m.x - m.width / 2
      : dir === 'right'
        ? c.x > m.x + m.width / 2
        : dir === 'up'
          ? c.y < m.y - m.height / 2
          : c.y > m.y + m.height / 2;
  const cands = attachPositions(m, others).filter(inDir);
  if (cands.length === 0) return null;
  return cands.reduce((best, c) => (dist(c, m) < dist(best, m) ? c : best));
}

export const replaceRect = (layout: ScreenLayout, id: string, r: Rect): ScreenLayout => ({
  screens: layout.screens.map((s) => (s.deviceId === id ? { ...s, x: r.x, y: r.y } : s)),
});

/** Which screens touch which (what edge switching uses), as [a, b] id pairs. */
export function adjacency(layout: ScreenLayout): [string, string][] {
  const out: [string, string][] = [];
  const s = layout.screens;
  for (let i = 0; i < s.length; i++) {
    for (let j = i + 1; j < s.length; j++) {
      const a = s[i]!;
      const b = s[j]!;
      const sideX = a.x + a.width === b.x || b.x + b.width === a.x;
      const sideY = a.y + a.height === b.y || b.y + b.height === a.y;
      const overlapY = a.y < b.y + b.height && b.y < a.y + a.height;
      const overlapX = a.x < b.x + b.width && b.x < a.x + a.width;
      if ((sideX && overlapY) || (sideY && overlapX)) out.push([a.deviceId, b.deviceId]);
    }
  }
  return out;
}
