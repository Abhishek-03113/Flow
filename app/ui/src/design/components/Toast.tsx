import { CircleAlert, CircleCheck } from 'lucide-react';
import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from 'react';

interface ToastItem {
  id: number;
  message: string;
  tone: 'info' | 'error';
}

interface ToastApi {
  show(message: string, tone?: ToastItem['tone']): void;
}

const Ctx = createContext<ToastApi>({ show: () => undefined });
export const useToast = (): ToastApi => useContext(Ctx);

/** Transient pill toasts (bottom centre). Announced politely; errors assertively. */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const seq = useRef(0);
  const show = useCallback((message: string, tone: ToastItem['tone'] = 'info') => {
    const id = ++seq.current;
    setItems((x) => [...x.slice(-2), { id, message, tone }]);
    setTimeout(() => setItems((x) => x.filter((i) => i.id !== id)), tone === 'error' ? 5000 : 2800);
  }, []);
  const api = useMemo(() => ({ show }), [show]);
  return (
    <Ctx.Provider value={api}>
      {children}
      <div className="fl-toasts" aria-live="polite" data-testid="toasts">
        {items.map((t) => (
          <div key={t.id} className="fl-toast" role={t.tone === 'error' ? 'alert' : 'status'}>
            {t.tone === 'error' ? (
              <CircleAlert size={16} color="var(--red)" aria-hidden />
            ) : (
              <CircleCheck size={16} color="var(--green)" aria-hidden />
            )}
            <span>{t.message}</span>
          </div>
        ))}
      </div>
    </Ctx.Provider>
  );
}
