/**
 * The engine's shortcut rules (`flow_core::switch_key`), for the mock engine:
 * a shortcut with a typing key needs Ctrl, Alt or Meta, and the return-home
 * key must differ from the switch key.
 */
type Mod = 'Ctrl' | 'Shift' | 'Alt' | 'Meta';

const MODIFIERS: Record<string, Mod> = {
  ctrl: 'Ctrl',
  control: 'Ctrl',
  shift: 'Shift',
  alt: 'Alt',
  option: 'Alt',
  opt: 'Alt',
  meta: 'Meta',
  cmd: 'Meta',
  command: 'Meta',
  win: 'Meta',
  windows: 'Meta',
  super: 'Meta',
};

const modifierOf = (token: string): Mod | null => MODIFIERS[token.toLowerCase()] ?? null;
const keyOf = (token: string) => token.toUpperCase().replace('SCROLL_LOCK', 'SCROLLLOCK');
/** Keys nobody types with: Scroll Lock, Pause, F1–F24. */
const STANDALONE = /^(SCROLLLOCK|PAUSE|F([1-9]|1\d|2[0-4]))$/;

/** `null` when `keys` work as a global shortcut. */
export function bindingProblem(keys: string[]): 'empty' | 'needs_modifier' | null {
  if (keys.length === 0) return 'empty';
  const commandModifier = keys.some((k) => {
    const m = modifierOf(k);
    return m !== null && m !== 'Shift';
  });
  const standaloneOnly = keys.filter((k) => !modifierOf(k)).every((k) => STANDALONE.test(keyOf(k)));
  return commandModifier || standaloneOnly ? null : 'needs_modifier';
}

/** The same keys, whatever their order or spelling. */
export function sameKeys(a: string[], b: string[]): boolean {
  const norm = (keys: string[]) => [...new Set(keys.map((k) => modifierOf(k) ?? keyOf(k)))].sort().join('+');
  return norm(a) === norm(b);
}

/** A remap rule's `from`, normalised the way the engine compares it. */
export const remapKey = (token: string): string => modifierOf(token) ?? keyOf(token);
