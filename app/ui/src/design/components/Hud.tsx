import type { ReactNode } from 'react';
import { cx } from '../cx';

/** macOS-style centred HUD (volume/brightness style). Non-interactive; announces via live region. */
export function Hud({
  icon,
  title,
  subtitle,
  leaving,
  inline,
}: {
  icon: ReactNode;
  title: string;
  subtitle?: string;
  leaving?: boolean;
  inline?: boolean;
}) {
  return (
    <div
      className={cx('fl-hud', leaving && 'fl-hud--out', inline && 'fl-hud--static')}
      role="status"
      aria-live="polite"
      data-testid="hud"
    >
      <div className="fl-hud__icon" aria-hidden>
        {icon}
      </div>
      <div>
        <p className="fl-hud__title">{title}</p>
        {subtitle && <p className="t-footnote t-secondary">{subtitle}</p>}
      </div>
    </div>
  );
}
