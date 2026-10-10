import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { cx } from "../cx";
import { Spinner } from "./Spinner";

export type ButtonVariant =
  | "bordered"
  | "prominent"
  | "plain"
  | "destructive"
  | "destructive-prominent";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: "small" | "regular" | "large";
  icon?: ReactNode;
  loading?: boolean;
  fullWidth?: boolean;
  /** Plain destructive text button (e.g. "Forget"). */
  tone?: "destructive";
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  function Button(
    {
      variant = "bordered",
      size = "regular",
      icon,
      loading,
      fullWidth,
      tone,
      className,
      children,
      disabled,
      ...rest
    },
    ref,
  ) {
    const iconOnly = !children;
    return (
      <button
        ref={ref}
        type="button"
        className={cx(
          "fl-btn",
          variant !== "bordered" && `fl-btn--${variant}`,
          size !== "regular" && `fl-btn--${size}`,
          fullWidth && "fl-btn--full",
          iconOnly && "fl-btn--icon",
          tone === "destructive" && "fl-btn--destructive",
          className,
        )}
        disabled={disabled || loading}
        aria-busy={loading || undefined}
        {...rest}
      >
        {loading ? <Spinner size={14} /> : icon}
        {children}
      </button>
    );
  },
);
