import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "../../lib/cn.ts";

export const ledVariants = cva("inline-block shrink-0 rounded-full", {
  variants: {
    state: {
      ok: "bg-ok text-ok led-glow",
      signal: "bg-signal text-signal led-glow",
      wait: "animate-breathe bg-warn",
      warn: "bg-warn text-warn led-glow",
      bad: "bg-bad text-bad led-glow",
      off: "bg-line-strong",
    },
    size: { sm: "size-1.5", md: "size-2", lg: "size-2.5" },
  },
  defaultVariants: { state: "off", size: "md" },
});

export type LedProps = VariantProps<typeof ledVariants> & { className?: string; label?: string };

/**
 * A status light. Decorative unless given a `label`: the word next to it is what
 * carries the meaning, so the colour never has to be told apart on its own.
 */
export function Led({ state, size, className, label }: LedProps) {
  return (
    <span
      className={cn(ledVariants({ state, size }), className)}
      {...(label ? { role: "img", "aria-label": label } : { "aria-hidden": true })}
    />
  );
}
