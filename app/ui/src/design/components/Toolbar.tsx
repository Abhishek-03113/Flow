import type { ReactNode } from 'react';

/** Window title region: large title on the left, actions on the right. Doubles as a window drag handle. */
export function Toolbar({
  title,
  subtitle,
  trailing,
}: {
  title: string;
  subtitle?: string;
  trailing?: ReactNode;
}) {
  return (
    <header className="fl-toolbar" data-tauri-drag-region>
      <div className="fl-toolbar__title" data-tauri-drag-region>
        <h1>{title}</h1>
        {subtitle && <p className="t-callout t-secondary">{subtitle}</p>}
      </div>
      {trailing}
    </header>
  );
}
