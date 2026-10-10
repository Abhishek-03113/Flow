import type { ReactNode } from "react";
import { cx } from "../cx";

export type Tone = "neutral" | "accent" | "green" | "orange" | "red";

export function Badge({
  tone = "neutral",
  children,
}: {
  tone?: Tone;
  children: ReactNode;
}) {
  return (
    <span className={cx("fl-badge", tone !== "neutral" && `fl-badge--${tone}`)}>
      {children}
    </span>
  );
}

export type DotTone = "gray" | "green" | "orange" | "red" | "accent";

/** Decorative status dot; pair it with text (never color alone). */
export function StatusDot({
  tone = "gray",
  pulse,
}: {
  tone?: DotTone;
  pulse?: boolean;
}) {
  return (
    <span
      aria-hidden
      className={cx(
        "fl-dot",
        tone !== "gray" && `fl-dot--${tone}`,
        pulse && "fl-dot--pulse",
      )}
    />
  );
}
