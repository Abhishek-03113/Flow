import { ChevronRight } from "lucide-react";
import type { ReactNode } from "react";
import { cx } from "../cx";

export function Section({
  title,
  footer,
  children,
  id,
}: {
  title?: string;
  footer?: ReactNode;
  children: ReactNode;
  id?: string;
}) {
  const titleId = id ? `${id}-title` : undefined;
  return (
    <section
      className="fl-section"
      aria-labelledby={title ? titleId : undefined}
      id={id}
    >
      {title && (
        <h2 className="fl-section__title" id={titleId}>
          {title}
        </h2>
      )}
      <ul className="fl-list">{children}</ul>
      {footer && <p className="fl-section__footer">{footer}</p>}
    </section>
  );
}

export type IconTone =
  | "neutral"
  | "accent"
  | "green"
  | "orange"
  | "red"
  | "gray";

export interface RowProps {
  icon?: ReactNode;
  iconTone?: IconTone;
  title: ReactNode;
  subtitle?: ReactNode;
  trailing?: ReactNode;
  /** Makes the whole row a button. */
  onClick?: () => void;
  chevron?: boolean;
  /** Extra content under the row's main line (inline notices, editors). */
  children?: ReactNode;
  "data-testid"?: string;
}

/** One inset-grouped row. Interactive trailing controls are siblings of the click target, never nested in it. */
export function Row({
  icon,
  iconTone = "neutral",
  title,
  subtitle,
  trailing,
  onClick,
  chevron,
  children,
  ...rest
}: RowProps) {
  const main = (
    <>
      {icon && (
        <span
          className={cx(
            "fl-row__icon",
            iconTone !== "neutral" && `fl-row__icon--${iconTone}`,
          )}
        >
          {icon}
        </span>
      )}
      <span className="fl-row__main">
        <span className="fl-row__title">{title}</span>
        {subtitle && (
          <span className="fl-row__subtitle" style={{ display: "block" }}>
            {subtitle}
          </span>
        )}
      </span>
    </>
  );
  return (
    <li className="fl-row-wrap" data-testid={rest["data-testid"]}>
      {onClick ? (
        <button
          type="button"
          className="fl-row fl-row--click"
          onClick={onClick}
        >
          {main}
          <span className="fl-row__trailing">
            {trailing}
            {chevron && (
              <ChevronRight
                size={14}
                strokeWidth={2}
                className="fl-row__chev"
                aria-hidden
              />
            )}
          </span>
        </button>
      ) : (
        <div className="fl-row">
          {main}
          {trailing && <span className="fl-row__trailing">{trailing}</span>}
        </div>
      )}
      {children}
    </li>
  );
}
