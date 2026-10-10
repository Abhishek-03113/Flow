import { cloneElement, useId, type ReactElement } from 'react';

/** Hover/focus label for icon-only controls. The child gets `aria-describedby`. */
export function Tooltip({
  label,
  children,
}: {
  label: string;
  children: ReactElement<{ 'aria-describedby'?: string }>;
}) {
  const id = useId();
  return (
    <span className="fl-tip">
      {cloneElement(children, { 'aria-describedby': id })}
      <span role="tooltip" id={id} className="fl-tip__bubble">
        {label}
      </span>
    </span>
  );
}
