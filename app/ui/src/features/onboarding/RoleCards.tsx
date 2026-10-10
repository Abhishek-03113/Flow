import type { Role } from '../../client/types';
import { ControlledArt, ControllerArt } from './Art';

const OPTIONS: { role: Role; title: string; text: string; art: React.ReactNode }[] = [
  {
    role: 'controller',
    title: "Share this computer's keyboard & mouse",
    text: 'Use the keyboard and mouse connected to this computer to control your other computers.',
    art: <ControllerArt />,
  },
  {
    role: 'controlled',
    title: 'Control this computer from another one',
    text: 'Let a computer with its own keyboard and mouse take over this one when you move to it.',
    art: <ControlledArt />,
  },
];

/** Radio group of two illustrated cards. Arrow keys move selection, like any radio group. */
export function RoleCards({ value, onChange }: { value: Role; onChange(r: Role): void }) {
  const onKey = (e: React.KeyboardEvent) => {
    if (['ArrowRight', 'ArrowDown', 'ArrowLeft', 'ArrowUp'].includes(e.key)) {
      e.preventDefault();
      const next: Role = value === 'controller' ? 'controlled' : 'controller';
      onChange(next);
      (e.currentTarget.querySelector(`[data-role="${next}"]`) as HTMLElement | null)?.focus();
    }
  };
  return (
    // eslint-disable-next-line jsx-a11y/interactive-supports-focus
    <div className="fl-roles" role="radiogroup" aria-label="What should this computer do?" onKeyDown={onKey}>
      {OPTIONS.map((o) => (
        <button
          key={o.role}
          type="button"
          role="radio"
          aria-checked={value === o.role}
          tabIndex={value === o.role ? 0 : -1}
          data-role={o.role}
          className="fl-rolecard"
          onClick={() => onChange(o.role)}
        >
          {o.art}
          <span className="fl-rolecard__title">{o.title}</span>
          <span className="t-callout t-secondary">{o.text}</span>
        </button>
      ))}
    </div>
  );
}
