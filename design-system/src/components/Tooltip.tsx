import { cloneElement, useId, type ReactElement } from "react";

/**
 * Hover/focus label for icon-only controls. The child gets `aria-describedby`.
 * `align="end"` right-aligns the bubble with the control, for controls at the
 * trailing edge of a row where a centred bubble would leave the window.
 */
export function Tooltip({
  label,
  align = "center",
  children,
}: {
  label: string;
  align?: "center" | "end";
  children: ReactElement<{ "aria-describedby"?: string }>;
}) {
  const id = useId();
  return (
    <span className="fl-tip">
      {cloneElement(children, { "aria-describedby": id })}
      <span
        role="tooltip"
        id={id}
        className={
          align === "end"
            ? "fl-tip__bubble fl-tip__bubble--end"
            : "fl-tip__bubble"
        }
      >
        {label}
      </span>
    </span>
  );
}
