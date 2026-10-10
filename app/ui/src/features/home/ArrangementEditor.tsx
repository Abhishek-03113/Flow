import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';
import type { Device, ScreenLayout } from '../../client/types';
import { cx } from '../../design';
import { DeviceGlyph } from '../DeviceGlyph';
import { nextSlot, replaceRect, snap, type Dir, type Rect } from './layoutMath';

interface Drag {
  id: string;
  startX: number;
  startY: number;
  raw: Rect;
  worldPerPx: number;
}

const ASPECT = 16 / 5;
const thisLabel = (os: Device['os']) =>
  os === 'macos' ? 'This Mac' : os === 'windows' ? 'This PC' : 'This computer';

/**
 * Drag device tiles like macOS Displays. Positions live in the engine's shared
 * virtual space; the view scales it to fit. Drops snap flush to an edge of
 * another screen and are persisted by `onCommit` (→ `set_screen_layout`).
 */
export function ArrangementEditor({
  layout,
  devices,
  activeId,
  editable,
  onCommit,
}: {
  layout: ScreenLayout;
  devices: Device[];
  /** DeviceId of the screen receiving input ("local" resolved to this computer's id by the caller). */
  activeId: string | null;
  editable: boolean;
  onCommit(next: ScreenLayout): Promise<boolean>;
}) {
  const [shown, setShown] = useState(layout);
  const [drag, setDrag] = useState<Drag | null>(null);
  const area = useRef<HTMLDivElement>(null);
  useEffect(() => setShown(layout), [layout]);

  const view = useMemo(() => {
    const rs = shown.screens;
    const minX = Math.min(...rs.map((r) => r.x));
    const minY = Math.min(...rs.map((r) => r.y));
    const maxX = Math.max(...rs.map((r) => r.x + r.width));
    const maxY = Math.max(...rs.map((r) => r.y + r.height));
    // Fit the group to ~70% of the canvas width (and 60% of its height).
    let vw = (maxX - minX) / 0.7;
    let vh = vw / ASPECT;
    if (maxY - minY > vh * 0.6) {
      vh = (maxY - minY) / 0.6;
      vw = vh * ASPECT;
    }
    return { x: (minX + maxX) / 2 - vw / 2, y: (minY + maxY) / 2 - vh / 2, w: vw, h: vh };
  }, [shown]);

  if (shown.screens.length === 0) return null;

  const pos = (r: Rect) => ({
    left: `${((r.x - view.x) / view.w) * 100}%`,
    top: `${((r.y - view.y) / view.h) * 100}%`,
    width: `${(r.width / view.w) * 100}%`,
    height: `${(r.height / view.h) * 100}%`,
  });
  const others = (id: string) => shown.screens.filter((s) => s.deviceId !== id);

  const commit = async (id: string, to: Rect) => {
    const before = shown;
    const next = replaceRect(shown, id, to);
    setShown(next);
    if (!(await onCommit(next))) setShown(before);
  };

  const onDown = (e: PointerEvent<HTMLDivElement>, r: Rect & { deviceId: string }) => {
    if (!editable) return;
    const box = area.current!.getBoundingClientRect();
    e.currentTarget.setPointerCapture(e.pointerId);
    setDrag({ id: r.deviceId, startX: e.clientX, startY: e.clientY, raw: r, worldPerPx: view.w / box.width });
  };
  const onMove = (e: PointerEvent<HTMLDivElement>) => {
    if (!drag) return;
    const s = shown.screens.find((x) => x.deviceId === drag.id)!;
    setDrag({
      ...drag,
      raw: {
        ...s,
        x: s.x + (e.clientX - drag.startX) * drag.worldPerPx,
        y: s.y + (e.clientY - drag.startY) * drag.worldPerPx,
      },
    });
  };
  const onUp = () => {
    if (!drag) return;
    const target = snap(drag.raw, others(drag.id));
    const cur = shown.screens.find((s) => s.deviceId === drag.id)!;
    const id = drag.id;
    setDrag(null);
    if (Math.round(target.x) !== cur.x || Math.round(target.y) !== cur.y) void commit(id, target);
  };
  const onKey = (e: KeyboardEvent, r: Rect & { deviceId: string }) => {
    const dir: Dir | undefined = (
      { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' } as const
    )[e.key as 'ArrowLeft'];
    if (!editable || !dir) return;
    e.preventDefault();
    const to = nextSlot(r, others(r.deviceId), dir);
    if (to) void commit(r.deviceId, to);
  };

  const ghost = drag ? snap(drag.raw, others(drag.id)) : null;

  return (
    <div
      ref={area}
      className="fl-arrange"
      data-testid="arrangement"
      role="group"
      aria-label="Screen arrangement"
    >
      {ghost && <div className="fl-ghost" style={pos(ghost)} />}
      {shown.screens.map((r) => {
        const d = devices.find((x) => x.id === r.deviceId);
        if (!d) return null;
        const dragging = drag?.id === r.deviceId;
        const rect = dragging ? drag.raw : r;
        return (
          // Tiles are keyboard-operable "buttons" that also support dragging.
          <div
            key={r.deviceId}
            role={editable ? 'button' : 'img'}
            tabIndex={editable ? 0 : undefined}
            aria-roledescription={editable ? 'movable screen' : undefined}
            aria-label={`${d.name}${d.isLocal ? ', this computer' : ''}${r.deviceId === activeId ? ', active' : ''}${
              editable ? '. Use arrow keys to move it next to another screen.' : ''
            }`}
            data-testid={`tile-${r.deviceId}`}
            className={cx(
              'fl-tile',
              editable && 'fl-tile--edit',
              dragging && 'fl-tile--drag',
              r.deviceId === activeId && 'fl-tile--active',
            )}
            style={pos(rect)}
            onPointerDown={(e) => onDown(e, r)}
            onPointerMove={onMove}
            onPointerUp={onUp}
            onPointerCancel={() => setDrag(null)}
            onKeyDown={(e) => onKey(e, r)}
          >
            <DeviceGlyph os={d.os} size={18} />
            <span className="fl-tile__name">{d.name}</span>
            <span className="fl-tile__meta">{d.isLocal ? thisLabel(d.os) : `${r.width}×${r.height}`}</span>
          </div>
        );
      })}
    </div>
  );
}
