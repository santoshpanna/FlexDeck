import { motion } from "motion/react";
import { useId } from "react";

import { cn } from "../../lib/cn.ts";
import { spring } from "../../motion.ts";

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
}

export interface SegmentedProps<T extends string> {
  options: readonly SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
  label: string;
  className?: string;
  size?: "sm" | "md";
}

/**
 * A row of choices in a recessed track, with ONE pill that springs to the
 * chosen one. The pill is a shared layout element, so it travels rather than
 * blinking between positions. `aria-pressed` carries the choice, not the fill.
 */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  label,
  className,
  size = "md",
}: SegmentedProps<T>) {
  const id = useId();
  return (
    <div
      role="group"
      aria-label={label}
      className={cn("inline-flex rounded-full border border-line bg-well p-[3px] shadow-well", className)}
    >
      {options.map((option) => {
        const on = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            aria-pressed={on}
            onClick={() => onChange(option.value)}
            className={cn(
              "relative cursor-pointer rounded-full font-mono tracking-[0.16em] uppercase transition-colors duration-150",
              size === "sm" ? "h-7 px-3 text-[10px]" : "h-8 px-3.5 text-[10px]",
              on ? "text-body" : "text-muted hover:text-ink",
            )}
          >
            {on ? (
              <motion.span
                layoutId={`seg-${id}`}
                transition={spring.panel}
                className="absolute inset-0 rounded-full bg-ink shadow-[0_2px_0_var(--fd-key-base)]"
              />
            ) : null}
            <span className="relative">{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}
