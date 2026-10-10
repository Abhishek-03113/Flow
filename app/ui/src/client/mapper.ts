/** The single snake_case (wire) <-> camelCase (TypeScript) key mapper. */

const toCamel = (s: string): string => s.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase());
const toSnake = (s: string): string => s.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`);

function mapKeys(value: unknown, fn: (k: string) => string): unknown {
  if (Array.isArray(value)) return value.map((v) => mapKeys(v, fn));
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [fn(k), mapKeys(v, fn)]));
  }
  return value;
}

/** wire -> TS. Only keys are rewritten; string values are untouched. */
export const fromWire = <T>(raw: unknown): T => mapKeys(raw, toCamel) as T;

/** TS -> wire. */
export const toWire = (value: unknown): unknown => mapKeys(value, toSnake);
