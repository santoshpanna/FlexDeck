import { cn } from "@flexdeck/ui";

import { Icon } from "../Icon/Icon.component.tsx";

export interface StepperProps {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
  className?: string;
}

const nudge =
  "flex size-8 cursor-pointer items-center justify-center rounded-lg text-muted transition-colors hover:bg-plate-2 hover:text-ink active:translate-y-px disabled:cursor-default disabled:opacity-35";

/**
 * A labelled number with − and + either side, in a recessed well.
 *
 * Clamped here rather than in the caller so no path can hand `onChange` a value
 * outside the range — not the buttons, and not a typed number. A non-finite
 * parse (someone mid-way through typing `-`) is ignored rather than written as `NaN`.
 */
export function Stepper({ label, value, min, max, onChange, className }: StepperProps) {
  const clamp = (candidate: number) => Math.min(Math.max(Math.trunc(candidate), min), max);
  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
      <span className="lbl">{label}</span>
      <div className="flex items-center rounded-[10px] border border-line bg-well p-0.5 shadow-well">
        <button type="button" className={nudge} onClick={() => onChange(clamp(value - 1))} disabled={value <= min} aria-label={`decrease ${label}`}>
          <Icon name="minus" size={14} decorative />
        </button>
        <input
          type="number"
          value={value}
          min={min}
          max={max}
          aria-label={label}
          className="w-full min-w-0 [appearance:textfield] bg-transparent text-center font-mono text-sm text-ink outline-none [&::-webkit-inner-spin-button]:m-0 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:m-0 [&::-webkit-outer-spin-button]:appearance-none"
          onChange={(event) => {
            const parsed = Number(event.target.value);
            if (Number.isFinite(parsed)) onChange(clamp(parsed));
          }}
        />
        <button type="button" className={nudge} onClick={() => onChange(clamp(value + 1))} disabled={value >= max} aria-label={`increase ${label}`}>
          <Icon name="plus" size={14} decorative />
        </button>
      </div>
    </div>
  );
}
