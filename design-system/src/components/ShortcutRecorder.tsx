import { useState, type KeyboardEvent } from "react";
import type { HostOs, ShortcutBinding } from "../types";
import { cx } from "../cx";
import {
  heldModifiers,
  isModifierToken,
  labelForKeys,
  tokenFromCode,
} from "../keys";
import { KeyCombo } from "./KeyCap";

export interface ShortcutRecorderProps {
  value: string[] | null;
  os: HostOs;
  onChange(binding: ShortcutBinding | null): void;
  label: string;
  /** `chord`: modifiers + one key. `key`: the first key pressed (modifiers allowed alone). */
  mode?: "chord" | "key";
  /** Backspace/Delete while recording clears the value. */
  allowClear?: boolean;
  emptyText?: string;
}

/**
 * Click, then press the shortcut. Esc cancels; focus loss cancels. Emits
 * canonical engine tokens (`Ctrl Alt Shift Meta`, `ScrollLock`, `F13`, `Space`, `A`…)
 * — never platform glyphs.
 */
export function ShortcutRecorder({
  value,
  os,
  onChange,
  label,
  mode = "chord",
  allowClear,
  emptyText = "Not set",
}: ShortcutRecorderProps) {
  const [recording, setRecording] = useState(false);
  const [held, setHeld] = useState<string[]>([]);

  const stop = () => {
    setRecording(false);
    setHeld([]);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    const mods = heldModifiers(e.nativeEvent);
    const token = tokenFromCode(e.code);
    if (e.key === "Escape" && mods.length === 0) return stop();
    if (
      (e.key === "Backspace" || e.key === "Delete") &&
      mods.length === 0 &&
      allowClear
    ) {
      stop();
      return onChange(null);
    }
    if (!token) return;
    if (mode === "key") {
      stop();
      return onChange({ keys: [token], label: labelForKeys([token]) });
    }
    if (isModifierToken(token)) return setHeld(mods);
    const keys = [...mods, token];
    stop();
    onChange({ keys, label: labelForKeys(keys) });
  };

  const onKeyUp = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (recording) setHeld(heldModifiers(e.nativeEvent));
  };

  return (
    <button
      type="button"
      className={cx("fl-recorder", recording && "fl-recorder--on")}
      aria-label={label}
      aria-pressed={recording}
      onClick={() => setRecording((r) => !r)}
      onBlur={stop}
      onKeyDown={onKeyDown}
      onKeyUp={onKeyUp}
    >
      {recording ? (
        <>
          {held.length > 0 && <KeyCombo keys={held} os={os} />}
          <span className="fl-recorder__hint">
            {mode === "key" ? "Press a key…" : "Type shortcut…"}
          </span>
        </>
      ) : value && value.length > 0 ? (
        <KeyCombo keys={value} os={os} />
      ) : (
        <span className="t-secondary">{emptyText}</span>
      )}
    </button>
  );
}
