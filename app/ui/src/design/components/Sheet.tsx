import { useId, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { useFocusTrap } from '../useFocusTrap';

export interface SheetProps {
  open: boolean;
  title: string;
  /** Escape / scrim click. Omit to make the sheet non-dismissible. */
  onClose?: () => void;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
  'data-testid'?: string;
}

/** Modal sheet, centred under the title bar like a macOS sheet. */
export function Sheet({ open, title, onClose, children, footer, width, ...rest }: SheetProps) {
  const titleId = useId();
  const ref = useFocusTrap<HTMLDivElement>(open, onClose);
  if (!open) return null;
  return createPortal(
    <div
      role="presentation"
      className="fl-scrim"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose?.();
      }}
    >
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className="fl-sheet"
        style={width ? { width } : undefined}
        data-testid={rest['data-testid']}
      >
        <div className="fl-sheet__head">
          <h2 id={titleId} className="t-title3">
            {title}
          </h2>
        </div>
        <div className="fl-sheet__body">{children}</div>
        {footer && <div className="fl-sheet__foot">{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}
