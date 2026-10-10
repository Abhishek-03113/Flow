import { useRef, type KeyboardEvent, type ReactNode } from "react";

export function Sidebar({
  children,
  footer,
  label = "Sections",
}: {
  children: ReactNode;
  footer?: ReactNode;
  label?: string;
}) {
  const nav = useRef<HTMLElement>(null);
  // Up/Down move focus between items (roving over the natural tab order).
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const items = Array.from(
      nav.current?.querySelectorAll<HTMLElement>(".fl-sidebar__item") ?? [],
    );
    const i = items.indexOf(document.activeElement as HTMLElement);
    if (i < 0) return;
    e.preventDefault();
    items[
      (i + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length
    ]?.focus();
  };
  return (
    <aside className="fl-sidebar" data-tauri-drag-region>
      <div style={{ height: "var(--s-6)" }} data-tauri-drag-region />
      {/* Arrow keys move between items; the handler is a convenience over the natural tab order. */}
      {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions */}
      <nav
        ref={nav}
        aria-label={label}
        className="fl-sidebar__nav"
        onKeyDown={onKeyDown}
      >
        {children}
      </nav>
      <div style={{ flex: 1 }} />
      {footer}
    </aside>
  );
}

export function SidebarItem({
  icon,
  label,
  active,
  onClick,
  badge,
  shortcut,
}: {
  icon: ReactNode;
  label: string;
  active?: boolean;
  onClick(): void;
  badge?: ReactNode;
  shortcut?: string;
}) {
  return (
    <button
      type="button"
      className="fl-sidebar__item"
      aria-current={active ? "page" : undefined}
      aria-label={label}
      aria-keyshortcuts={shortcut}
      onClick={onClick}
    >
      <span className="fl-sidebar__glyph" aria-hidden>
        {icon}
      </span>
      <span className="fl-sidebar__label">{label}</span>
      {badge}
    </button>
  );
}
