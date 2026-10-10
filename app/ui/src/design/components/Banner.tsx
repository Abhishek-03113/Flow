import type { ReactNode } from 'react';
import { cx } from '../cx';

export function Banner({
  tone = 'gray',
  icon,
  title,
  children,
  actions,
  role = 'status',
}: {
  tone?: 'gray' | 'orange' | 'red';
  icon?: ReactNode;
  title: string;
  children?: ReactNode;
  actions?: ReactNode;
  role?: 'status' | 'alert';
}) {
  return (
    <div className={cx('fl-banner', `fl-banner--${tone}`)} role={role} data-testid="banner">
      {icon && <span className="fl-banner__icon">{icon}</span>}
      <div className="fl-banner__text">
        <p className="fl-banner__title">{title}</p>
        {children && <p className="fl-banner__detail">{children}</p>}
      </div>
      {actions && <div className="fl-banner__actions">{actions}</div>}
    </div>
  );
}
