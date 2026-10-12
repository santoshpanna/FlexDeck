import type { ReactNode } from "react";

import { cn } from "../../lib/cn.ts";

/** A small neutral label beside a name: what kind of thing a row is. Never coloured. */
export function Pill({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center rounded-md border border-line px-1.5 py-0.5 font-mono text-[9px] leading-none tracking-[0.14em] text-muted uppercase",
        className,
      )}
    >
      {children}
    </span>
  );
}
