import { Check, Copy } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "./Button";

/**
 * A copyable shell command: monospace text on a tertiary surface with a Copy
 * button. The button label announces the command; after copying it says
 * "Copied" for two seconds.
 */
export function CodeBlock({
  children,
  onCopy,
}: {
  /** The command text. */
  children: string;
  /** Called after the clipboard write (e.g. to show a toast). */
  onCopy?: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  useEffect(() => () => clearTimeout(timer.current), []);

  const copy = () => {
    void navigator.clipboard?.writeText(children);
    setCopied(true);
    onCopy?.();
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="fl-codeblock">
      <code className="fl-codeblock__code">{children}</code>
      <Button
        size="small"
        aria-label={copied ? "Copied" : `Copy command: ${children}`}
        icon={
          copied ? (
            <Check size={13} strokeWidth={1.8} />
          ) : (
            <Copy size={13} strokeWidth={1.6} />
          )
        }
        onClick={copy}
      >
        {copied ? "Copied" : "Copy"}
      </Button>
    </div>
  );
}
