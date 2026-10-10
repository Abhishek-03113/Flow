import type { HostOs } from "./types";

/** Canonical key tokens (what the engine's hotkey matcher understands). */
const MODIFIER_CODES: Record<string, string> = {
  ControlLeft: "Ctrl",
  ControlRight: "Ctrl",
  AltLeft: "Alt",
  AltRight: "Alt",
  ShiftLeft: "Shift",
  ShiftRight: "Shift",
  MetaLeft: "Meta",
  MetaRight: "Meta",
};

const MODIFIER_ORDER = ["Ctrl", "Alt", "Shift", "Meta"];

const NAMED: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  Escape: "Esc",
  ScrollLock: "ScrollLock",
  Pause: "Pause",
  CapsLock: "CapsLock",
  NumLock: "NumLock",
  PrintScreen: "PrintScreen",
  Insert: "Insert",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
};

/** `KeyboardEvent.code` → canonical token, or null for keys we cannot name. */
export function tokenFromCode(code: string): string | null {
  if (MODIFIER_CODES[code]) return MODIFIER_CODES[code]!;
  if (NAMED[code]) return NAMED[code]!;
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  if (/^Numpad/.test(code)) return code.replace("Numpad", "Num");
  return null;
}

export const isModifierToken = (t: string): boolean =>
  MODIFIER_ORDER.includes(t);

/** Modifiers held in a keyboard event, in canonical order. */
export function heldModifiers(
  e: Pick<KeyboardEvent, "ctrlKey" | "altKey" | "shiftKey" | "metaKey">,
): string[] {
  return [
    e.ctrlKey && "Ctrl",
    e.altKey && "Alt",
    e.shiftKey && "Shift",
    e.metaKey && "Meta",
  ].filter((x): x is string => Boolean(x));
}

/** Human label in the engine's platform-neutral spelling, e.g. `Ctrl + Shift + Space`. */
export function labelForKeys(keys: string[]): string {
  const pretty = (k: string) =>
    k === "ScrollLock" ? "Scroll Lock" : k === "CapsLock" ? "Caps Lock" : k;
  return keys.map(pretty).join(" + ");
}

const MAC_GLYPHS: Record<string, string> = {
  Ctrl: "⌃",
  Alt: "⌥",
  Shift: "⇧",
  Meta: "⌘",
  Enter: "↩",
  Backspace: "⌫",
  Delete: "⌦",
  Tab: "⇥",
  Esc: "⎋",
  Up: "↑",
  Down: "↓",
  Left: "←",
  Right: "→",
  CapsLock: "⇪",
};

export interface KeyDisplay {
  text: string;
  /** True when `text` is a symbol glyph (rendered larger). */
  glyph: boolean;
}

/** How a key token is drawn on a key cap on the given OS. */
export function displayKey(token: string, os: HostOs): KeyDisplay {
  if (os === "macos" && MAC_GLYPHS[token])
    return { text: MAC_GLYPHS[token]!, glyph: true };
  if (token === "Meta")
    return { text: os === "windows" ? "Win" : "Super", glyph: false };
  if (token === "ScrollLock") return { text: "Scroll Lock", glyph: false };
  if (token === "CapsLock") return { text: "Caps Lock", glyph: false };
  if (token === "Esc") return { text: "Esc", glyph: false };
  if (os !== "macos") {
    const arrows: Record<string, string> = {
      Up: "↑",
      Down: "↓",
      Left: "←",
      Right: "→",
    };
    if (arrows[token]) return { text: arrows[token]!, glyph: true };
  }
  return {
    text: token.length === 1 ? token.toUpperCase() : token,
    glyph: false,
  };
}

/** Accessible spoken form of a chord: "Control Shift Space". */
export function spokenKeys(keys: string[], os: HostOs): string {
  const spoken: Record<string, string> = {
    Ctrl: "Control",
    Alt: os === "macos" ? "Option" : "Alt",
    Meta: os === "macos" ? "Command" : os === "windows" ? "Windows" : "Super",
    ScrollLock: "Scroll Lock",
  };
  return keys.map((k) => spoken[k] ?? k).join(" ");
}
