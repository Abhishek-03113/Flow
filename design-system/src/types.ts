/** Operating system used to pick key glyphs (⌘ vs Win) and platform wording. */
export type HostOs = "macos" | "windows" | "linux";

/** A recorded shortcut: canonical key tokens plus a human label, e.g. `{ label: 'Ctrl + Shift + Space', keys: ['Ctrl','Shift','Space'] }`. */
export interface ShortcutBinding {
  label: string;
  keys: string[];
}
