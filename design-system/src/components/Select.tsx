import { ChevronsUpDown } from "lucide-react";
import type { SelectHTMLAttributes } from "react";

export interface SelectOption<T extends string> {
  value: T;
  label: string;
}

export interface SelectProps<T extends string> extends Omit<
  SelectHTMLAttributes<HTMLSelectElement>,
  "onChange" | "value"
> {
  value: T;
  options: SelectOption<T>[];
  onChange(v: T): void;
  label: string;
}

/** Pop-up button. A native `<select>` keeps full keyboard + screen-reader behaviour. */
export function Select<T extends string>({
  value,
  options,
  onChange,
  label,
  ...rest
}: SelectProps<T>) {
  return (
    <span className="fl-select">
      <select
        aria-label={label}
        value={value}
        onChange={(e) => onChange(e.target.value as T)}
        {...rest}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      <ChevronsUpDown
        className="fl-select__chev"
        size={12}
        strokeWidth={1.75}
        aria-hidden
      />
    </span>
  );
}
