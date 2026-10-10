import { createContext, useContext, useMemo, useState, type ReactNode } from 'react';

interface UiApi {
  pairingOpen: boolean;
  openPairing(): void;
  closePairing(): void;
  permissionOpen: boolean;
  openPermission(): void;
  closePermission(): void;
  navigate(path: string): void;
}

const Ctx = createContext<UiApi | null>(null);

export function useUi(): UiApi {
  const c = useContext(Ctx);
  if (!c) throw new Error('useUi must be used inside <UiProvider>');
  return c;
}

/** Global UI affordances (sheets that any screen can open, navigation). */
export function UiProvider({ navigate, children }: { navigate(path: string): void; children: ReactNode }) {
  const [pairingOpen, setPairing] = useState(false);
  const [permissionOpen, setPermission] = useState(false);
  const value = useMemo<UiApi>(
    () => ({
      pairingOpen,
      openPairing: () => setPairing(true),
      closePairing: () => setPairing(false),
      permissionOpen,
      openPermission: () => setPermission(true),
      closePermission: () => setPermission(false),
      navigate,
    }),
    [pairingOpen, permissionOpen, navigate],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
