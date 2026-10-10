import { useEffect } from 'react';
import { TAURI_EVENT_NAVIGATE, isTauriRuntime } from '../../client/tauriTransport';
import { useUi } from '../../state/ui';

/** What the tray popover (or a notification) asks the main window to show. */
interface NavigateRequest {
  route?: string;
  sheet?: 'pairing' | 'permission';
}

/** Desktop shell → main window: open a route and/or a sheet. Renders nothing. */
export function ShellNavigation() {
  const { navigate, openPairing, openPermission } = useUi();
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let off: (() => void) | null = null;
    let stopped = false;
    void import('@tauri-apps/api/event').then(async ({ listen }) => {
      const un = await listen<NavigateRequest>(TAURI_EVENT_NAVIGATE, (e) => {
        if (e.payload.route) navigate(e.payload.route);
        if (e.payload.sheet === 'pairing') openPairing();
        if (e.payload.sheet === 'permission') openPermission();
      });
      if (stopped) un();
      else off = un;
    });
    return () => {
      stopped = true;
      off?.();
    };
  }, [navigate, openPairing, openPermission]);
  return null;
}
