import { motion, type HTMLMotionProps } from "motion/react";
import { forwardRef, type ReactNode } from "react";

import { cn } from "../../lib/cn.ts";
import { spring } from "../../motion.ts";
import { keyVariants, type KeyVariants } from "./key.variants.ts";

export type KeyProps = Omit<HTMLMotionProps<"button">, "children"> &
  KeyVariants & {
    children?: ReactNode;
    /** Draw a status LED in the key's corner. */
    led?: "ok" | "signal" | "warn" | "off" | null;
  };

/** A pressable key. `type="button"` unless told otherwise, so a key in a form never submits by accident. */
export const Key = forwardRef<HTMLButtonElement, KeyProps>(function Key(
  { className, tone, size, lit, led = null, type = "button", children, ...rest },
  ref,
) {
  return (
    <motion.button
      ref={ref}
      type={type}
      whileTap={{ y: 3, scale: 0.98 }}
      transition={spring.key}
      className={cn(keyVariants({ tone, size, lit }), className)}
      {...rest}
    >
      {led ? (
        <span
          aria-hidden="true"
          className={cn(
            "absolute top-2 right-2 size-1.5 rounded-full",
            led === "ok" && "bg-ok text-ok led-glow",
            led === "signal" && "bg-signal text-signal led-glow",
            led === "warn" && "animate-breathe bg-warn",
            led === "off" && "bg-line-strong",
          )}
        />
      ) : null}
      {children}
    </motion.button>
  );
});
