import type { ButtonHTMLAttributes } from "react";

import { cn, keyVariants } from "@flexdeck/ui";

export type ButtonVariant = "primary" | "quiet" | "link";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
}

/**
 * The portal's button, drawn as a key from the shared kit. `primary` is the
 * signal key — one per surface, for the thing to do next; `quiet` an ordinary
 * key; `link` an inline text action.
 */
export function Button({ variant = "primary", className, type = "button", ...rest }: ButtonProps) {
  if (variant === "link") {
    return (
      <button
        type={type}
        className={cn("cursor-pointer text-signal underline-offset-4 hover:underline disabled:opacity-50", className)}
        {...rest}
      />
    );
  }
  return (
    <button
      type={type}
      className={cn(
        keyVariants({ tone: variant === "primary" ? "signal" : "default", size: "md" }),
        "transition-transform active:translate-y-[2px]",
        className,
      )}
      {...rest}
    />
  );
}
