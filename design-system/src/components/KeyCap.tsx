import type { HostOs } from "../types";
import { cx } from "../cx";
import { displayKey, spokenKeys } from "../keys";

export function KeyCap({
  token,
  os,
  large,
}: {
  token: string;
  os: HostOs;
  large?: boolean;
}) {
  const d = displayKey(token, os);
  return (
    <kbd
      className={cx(
        "fl-keycap",
        d.glyph && "fl-keycap--glyph",
        large && "fl-keycap--lg",
      )}
    >
      {d.text}
    </kbd>
  );
}

/** A chord drawn as key caps: ⌘ on macOS, text elsewhere. Reads aloud as words. */
export function KeyCombo({
  keys,
  os,
  large,
}: {
  keys: string[];
  os: HostOs;
  large?: boolean;
}) {
  return (
    <span className="fl-keycombo" role="img" aria-label={spokenKeys(keys, os)}>
      {keys.map((k, i) => (
        <KeyCap key={`${k}-${i}`} token={k} os={os} large={large} />
      ))}
    </span>
  );
}
