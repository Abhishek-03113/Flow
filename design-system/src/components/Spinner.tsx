export function Spinner({
  size = 16,
  label,
}: {
  size?: number;
  label?: string;
}) {
  return (
    <span
      className="fl-spinner"
      style={{ "--size": `${size}px` } as React.CSSProperties}
      role={label ? "status" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    />
  );
}
