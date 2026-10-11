import { motion } from "motion/react";
import { useId } from "react";

import { cn } from "../../lib/cn.ts";
import { spring } from "../../motion.ts";
import { Glyph, type GlyphName } from "../Icon/Glyph.component.tsx";

export interface TabBarItem<T extends string> {
  value: T;
  label: string;
  glyph: GlyphName;
  /** A count or a dot that needs attention, shown on the key. */
  badge?: number | null;
}

/**
 * The window's bottom row of keys, sitting in a well. One raised cap springs to
 * the current tab — a single shared layout element, so it slides rather than
 * reappearing.
 */
export function TabBar<T extends string>({
  items,
  value,
  onChange,
  className,
}: {
  items: readonly TabBarItem<T>[];
  value: T;
  onChange: (value: T) => void;
  className?: string;
}) {
  const id = useId();
  return (
    <nav
      aria-label="Sections"
      className={cn(
        "grid rounded-[18px] border border-line bg-well p-1.5 shadow-well",
        className,
      )}
      style={{ gridTemplateColumns: `repeat(${items.length}, minmax(0, 1fr))` }}
    >
      {items.map((item) => {
        const on = item.value === value;
        return (
          <button
            key={item.value}
            type="button"
            aria-current={on ? "page" : undefined}
            onClick={() => onChange(item.value)}
            className={cn(
              "relative flex h-[52px] cursor-pointer flex-col items-center justify-center gap-1 rounded-xl transition-colors duration-200",
              on ? "text-ink" : "text-muted hover:text-ink-2",
            )}
          >
            {on ? (
              <motion.span
                layoutId={`tab-${id}`}
                transition={spring.panel}
                className="absolute inset-0 rounded-xl border border-[var(--fd-key-line)] bg-linear-to-b from-[var(--fd-key-top)] to-[var(--fd-key-bottom)] shadow-[inset_0_1px_0_var(--fd-key-glint),0_2px_0_var(--fd-key-base)]"
              />
            ) : null}
            <span className="relative">
              <Glyph name={item.glyph} size={18} />
              {item.badge ? (
                <span className="absolute -top-1 -right-2 size-2 rounded-full bg-signal text-signal led-glow" />
              ) : null}
            </span>
            <span className="relative font-mono text-[9px] tracking-[0.18em]">{item.label}</span>
          </button>
        );
      })}
    </nav>
  );
}
