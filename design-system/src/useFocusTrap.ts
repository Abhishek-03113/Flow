import { useEffect, useRef } from "react";

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * Modal focus management: focus moves into the dialog on open (to
 * `[data-autofocus]` when present), Tab/Shift+Tab wrap, Escape closes, and
 * focus is restored to the previously focused element on close.
 */
export function useFocusTrap<T extends HTMLElement>(
  open: boolean,
  onClose?: () => void,
) {
  const ref = useRef<T>(null);
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    const node = ref.current;
    const items = () =>
      Array.from(node?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? []);
    (
      node?.querySelector<HTMLElement>("[data-autofocus]") ??
      items()[0] ??
      node
    )?.focus();

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && close.current) {
        e.stopPropagation();
        close.current();
        return;
      }
      if (e.key !== "Tab") return;
      const list = items();
      if (list.length === 0) return e.preventDefault();
      const first = list[0]!;
      const last = list[list.length - 1]!;
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      previous?.focus?.();
    };
  }, [open]);

  return ref;
}
