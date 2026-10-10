import { useCallback, useState } from 'react';
import { useToast } from '../design';
import { friendlyError } from '../copy';

/**
 * Runs an engine command, tracks `busy`, and turns failures into a friendly
 * error toast. Resolves to true on success.
 */
export function useAction() {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const run = useCallback(
    async (fn: () => Promise<unknown>, successMessage?: string): Promise<boolean> => {
      setBusy(true);
      try {
        await fn();
        if (successMessage) toast.show(successMessage);
        return true;
      } catch (e) {
        toast.show(friendlyError(e), 'error');
        return false;
      } finally {
        setBusy(false);
      }
    },
    [toast],
  );
  return { run, busy };
}
