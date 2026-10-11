import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import { cn } from "../../lib/cn.ts";
import { spring } from "../../motion.ts";

// Cells share the row's width and shrink with it — up to their full size — so
// a code always fits a narrow window. Each cell is a size container, and its
// character is sized against the cell rather than the window.
const CELL = {
  lg: "max-w-16 aspect-[64/84] rounded-xl",
  sm: "max-w-[34px] aspect-[34/56] rounded-lg",
} as const;

// On the character, not the cell: container units resolve against the nearest
// *ancestor* container, so on the cell itself they would read the window.
const CHAR = {
  lg: "text-[69cqw]",
  sm: "text-[76cqw]",
} as const;

/**
 * A code shown the way a device shows one: each character in its own recessed
 * cell, glowing, and flipping in like a split-flap when it changes. A new code
 * reads as new without anyone comparing digits.
 */
export function Digits({
  value,
  size = "lg",
  tone = "signal",
  className,
}: {
  value: string;
  size?: keyof typeof CELL;
  tone?: "signal" | "ok";
  className?: string;
}) {
  const reduce = useReducedMotion();
  return (
    <div
      className={cn("flex w-full justify-center", size === "lg" ? "gap-1.5 min-[440px]:gap-2.5" : "gap-1", className)}
      aria-label={value}
      role="img"
    >
      {value.split("").map((char, index) =>
        char === "-" || char === " " ? (
          <span key={index} aria-hidden="true" className="self-center px-0.5 font-mono text-xl text-faint">
            –
          </span>
        ) : (
          <span
            key={index}
            aria-hidden="true"
            className={cn(
              "relative flex min-w-0 flex-1 items-center justify-center overflow-hidden @container border border-line bg-well shadow-well [perspective:300px]",
              "after:absolute after:inset-x-0 after:top-1/2 after:h-px after:bg-[var(--fd-well-shade)]",
              CELL[size],
            )}
          >
            <AnimatePresence mode="popLayout" initial={false}>
              <motion.span
                key={`${index}-${char}`}
                initial={reduce ? { opacity: 0 } : { rotateX: 90, opacity: 0 }}
                animate={reduce ? { opacity: 1 } : { rotateX: 0, opacity: 1 }}
                exit={reduce ? { opacity: 0 } : { rotateX: -90, opacity: 0 }}
                transition={{ ...spring.key, delay: index * 0.05 }}
                className={cn(
                  "font-mono leading-none font-medium",
                  CHAR[size],
                  tone === "signal"
                    ? "text-signal-hi [text-shadow:0_0_14px_color-mix(in_oklab,var(--color-signal)_60%,transparent)]"
                    : "text-ok [text-shadow:0_0_14px_color-mix(in_oklab,var(--color-ok)_50%,transparent)]",
                )}
              >
                {char}
              </motion.span>
            </AnimatePresence>
          </span>
        ),
      )}
    </div>
  );
}
