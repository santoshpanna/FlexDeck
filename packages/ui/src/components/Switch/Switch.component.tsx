import { motion } from "motion/react";

import { cn } from "../../lib/cn.ts";
import { spring } from "../../motion.ts";

export interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
  size?: "sm" | "md";
  className?: string;
}

/** A machined toggle: the knob springs across and lands with a small overshoot. */
export function Switch({ checked, onChange, label, disabled, size = "md", className }: SwitchProps) {
  const small = size === "sm";
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative shrink-0 cursor-pointer rounded-full border shadow-well transition-colors duration-200 disabled:cursor-default disabled:opacity-45",
        small ? "h-5 w-9" : "h-[26px] w-[46px]",
        checked ? "border-signal-line bg-signal-soft" : "border-line bg-well",
        className,
      )}
    >
      <motion.span
        aria-hidden="true"
        initial={false}
        animate={{ x: checked ? (small ? 16 : 20) : 0 }}
        transition={spring.knob}
        className={cn(
          "absolute top-[2px] left-[2px] rounded-full bg-linear-to-b from-[#f4f3ee] to-[#bebdb7] shadow-[0_2px_4px_rgb(0_0_0/0.45)]",
          small ? "size-3.5" : "size-5",
        )}
      />
    </button>
  );
}
