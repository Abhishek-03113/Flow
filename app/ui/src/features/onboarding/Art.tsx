/** CSS/SVG illustrations (no raster assets). Colours come from design tokens via currentColor. */

const stroke = {
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round',
  strokeLinejoin: 'round',
} as const;

/** Two screens joined by a flowing line, with a cursor crossing between them. */
export function WelcomeArt() {
  return (
    <svg
      className="fl-hero"
      viewBox="0 0 220 140"
      role="img"
      aria-label="Two computers sharing one keyboard and mouse"
    >
      <rect x="10" y="30" width="78" height="54" rx="7" {...stroke} />
      <path d="M36 98h26M49 84v14" {...stroke} />
      <rect x="132" y="30" width="78" height="54" rx="7" {...stroke} fill="var(--accent-soft)" />
      <path d="M158 98h26M171 84v14" {...stroke} />
      <path d="M88 57c14-22 30 22 44 0" {...stroke} strokeDasharray="1 6" />
      <path d="M150 46l0 15 4-3.5 3 7 3-1.5-3-6.5 5.5-.5z" fill="currentColor" stroke="none" />
      <rect x="82" y="112" width="56" height="16" rx="5" {...stroke} />
      <path d="M92 120h8M104 120h8M116 120h12" {...stroke} strokeWidth={1.5} />
    </svg>
  );
}

/** A keyboard and mouse sending to a screen. */
export function ControllerArt() {
  return (
    <svg
      className="fl-rolecard__art"
      viewBox="0 0 200 96"
      role="img"
      aria-label="Keyboard and mouse sending input to a screen"
    >
      <rect x="8" y="48" width="84" height="34" rx="7" {...stroke} />
      <path d="M20 60h6M32 60h6M44 60h6M56 60h6M68 60h6M20 71h54" {...stroke} strokeWidth={1.5} />
      <rect x="104" y="46" width="22" height="36" rx="11" {...stroke} />
      <path d="M115 46v14" {...stroke} strokeWidth={1.5} />
      <path d="M134 62h18" {...stroke} strokeDasharray="1 5" />
      <path d="M150 56l8 6-8 6" {...stroke} />
      <rect x="164" y="30" width="30" height="40" rx="5" {...stroke} fill="var(--accent-soft)" />
    </svg>
  );
}

/** A screen receiving a cursor from elsewhere. */
export function ControlledArt() {
  return (
    <svg
      className="fl-rolecard__art"
      viewBox="0 0 200 96"
      role="img"
      aria-label="A computer being controlled by another one"
    >
      <rect x="6" y="30" width="30" height="40" rx="5" {...stroke} />
      <path d="M44 50h18" {...stroke} strokeDasharray="1 5" />
      <path d="M60 44l8 6-8 6" {...stroke} />
      <rect x="82" y="16" width="108" height="64" rx="8" {...stroke} fill="var(--accent-soft)" />
      <path d="M122 84h28" {...stroke} />
      <path d="M126 36l0 20 5-4.5 4 9 4-2-4-8.5 7-.5z" fill="currentColor" stroke="none" />
    </svg>
  );
}
