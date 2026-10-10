import type { CSSProperties } from 'react';

export interface SliderProps {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange(v: number): void;
  label: string;
  disabled?: boolean;
  /** Spoken value, e.g. "Instant" or "250 milliseconds". */
  valueText?: string;
}

export function Slider({ value, min, max, step = 1, onChange, label, disabled, valueText }: SliderProps) {
  const fill = ((value - min) / (max - min)) * 100;
  return (
    <input
      type="range"
      className="fl-slider"
      min={min}
      max={max}
      step={step}
      value={value}
      disabled={disabled}
      aria-label={label}
      aria-valuetext={valueText}
      style={{ '--fill': `${fill}%` } as CSSProperties}
      onChange={(e) => onChange(Number(e.target.value))}
    />
  );
}
