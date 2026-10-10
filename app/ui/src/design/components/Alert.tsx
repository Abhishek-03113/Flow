import { useId } from 'react';
import { createPortal } from 'react-dom';
import { useFocusTrap } from '../useFocusTrap';
import { Button, type ButtonVariant } from './Button';

export interface AlertAction {
  label: string;
  variant?: ButtonVariant;
  onClick(): void;
  /** Receives initial focus (the safe choice, never the destructive one). */
  autoFocus?: boolean;
}

export interface AlertProps {
  open: boolean;
  title: string;
  message: string;
  actions: AlertAction[];
  onClose(): void;
}

/** macOS-style alert: icon-less, centred, stacked full-width buttons. */
export function Alert({ open, title, message, actions, onClose }: AlertProps) {
  const id = useId();
  const ref = useFocusTrap<HTMLDivElement>(open, onClose);
  if (!open) return null;
  return createPortal(
    <div className="fl-scrim fl-scrim--center">
      <div
        ref={ref}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={`${id}-t`}
        aria-describedby={`${id}-m`}
        tabIndex={-1}
        className="fl-alert"
      >
        <h2 id={`${id}-t`} className="fl-alert__title">
          {title}
        </h2>
        <p id={`${id}-m`} className="fl-alert__msg">
          {message}
        </p>
        <div className="fl-alert__actions">
          {actions.map((a) => (
            <Button
              key={a.label}
              variant={a.variant}
              fullWidth
              onClick={a.onClick}
              data-autofocus={a.autoFocus ? '' : undefined}
            >
              {a.label}
            </Button>
          ))}
        </div>
      </div>
    </div>,
    document.body,
  );
}
