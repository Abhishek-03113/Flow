import { useCallback, useEffect, useState } from 'react';

/** Minimal hash router: `#/devices` → `/devices`. Hash routes work under Tauri and `vite preview` alike. */
export function useHashRoute(): [string, (path: string) => void] {
  const read = () => window.location.hash.replace(/^#/, '') || '/';
  const [route, setRoute] = useState(read);
  useEffect(() => {
    const on = () => setRoute(read());
    window.addEventListener('hashchange', on);
    return () => window.removeEventListener('hashchange', on);
  }, []);
  const go = useCallback((path: string) => {
    window.location.hash = path;
  }, []);
  return [route, go];
}
