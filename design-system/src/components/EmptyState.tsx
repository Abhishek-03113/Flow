import type { ReactNode } from "react";

export function EmptyState({
  art,
  title,
  children,
  actions,
}: {
  art?: ReactNode;
  title: string;
  children?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <div className="fl-empty">
      {art && <div className="fl-empty__art">{art}</div>}
      <h2 className="fl-empty__title">{title}</h2>
      {children && <p className="fl-empty__text">{children}</p>}
      {actions && <div className="fl-empty__actions">{actions}</div>}
    </div>
  );
}
