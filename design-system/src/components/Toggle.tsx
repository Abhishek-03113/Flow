export interface ToggleProps {
  checked: boolean;
  onChange(next: boolean): void;
  label: string;
  disabled?: boolean;
  id?: string;
}

/** Switch. `label` is the accessible name (visible text lives in the surrounding Row). */
export function Toggle({
  checked,
  onChange,
  label,
  disabled,
  id,
}: ToggleProps) {
  return (
    <button
      id={id}
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className="fl-toggle"
      onClick={() => onChange(!checked)}
    />
  );
}
