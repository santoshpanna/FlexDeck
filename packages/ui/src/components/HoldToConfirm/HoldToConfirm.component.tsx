import { animate, motion, useMotionValue } from "motion/react";
import { useRef, type ReactNode } from "react";

import { cn } from "../../lib/cn.ts";
import { spring } from "../../motion.ts";
import { keyVariants, type KeyVariants } from "../Key/key.variants.ts";

export interface HoldToConfirmProps extends KeyVariants {
  onConfirm: () => void;
  children: ReactNode;
  /** How long the press must last, in milliseconds. */
  hold?: number;
  disabled?: boolean;
  className?: string;
}

/**
 * For things worth one more second: press and hold until the fill reaches the
 * end. Letting go early drains it back. Keyboard users get the same deliberate
 * step — Enter or Space held — rather than a bypass.
 */
export function HoldToConfirm({ onConfirm, children, hold = 900, disabled, className, tone, size }: HoldToConfirmProps) {
  const progress = useMotionValue(0);
  const running = useRef<ReturnType<typeof animate> | null>(null);

  const start = () => {
    if (disabled) return;
    running.current?.stop();
    running.current = animate(progress, 1, {
      duration: (hold / 1000) * (1 - progress.get()),
      ease: "linear",
      onComplete: () => {
        progress.set(0);
        onConfirm();
      },
    });
  };
  const cancel = () => {
    if (progress.get() === 0) return;
    running.current?.stop();
    running.current = animate(progress, 0, { duration: 0.2 });
  };

  return (
    <motion.button
      type="button"
      disabled={disabled}
      whileTap={{ y: 3, scale: 0.98 }}
      transition={spring.key}
      onPointerDown={start}
      onPointerUp={cancel}
      onPointerLeave={cancel}
      onKeyDown={(event) => {
        if ((event.key === "Enter" || event.key === " ") && !event.repeat) {
          event.preventDefault();
          start();
        }
      }}
      onKeyUp={(event) => {
        if (event.key === "Enter" || event.key === " ") cancel();
      }}
      className={cn(keyVariants({ tone, size }), "overflow-hidden", className)}
    >
      <motion.span aria-hidden="true" style={{ scaleX: progress }} className="absolute inset-0 origin-left bg-white/25" />
      <span className="relative">{children}</span>
    </motion.button>
  );
}
