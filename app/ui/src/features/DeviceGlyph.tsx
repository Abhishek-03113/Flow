import { Laptop, Monitor, SquareTerminal } from 'lucide-react';
import type { HostOs } from '../client/types';

/** One glyph per OS family (SF Symbols can't be redistributed; thin-stroke Lucide stands in). */
export function DeviceGlyph({ os, size = 16 }: { os: HostOs; size?: number }) {
  const p = { size, strokeWidth: 1.6, 'aria-hidden': true } as const;
  return os === 'macos' ? (
    <Laptop {...p} />
  ) : os === 'windows' ? (
    <Monitor {...p} />
  ) : (
    <SquareTerminal {...p} />
  );
}
