import { useRef, useState, type ReactNode } from "react";

import { cn } from "../../lib/cn.ts";
import { Glyph } from "../Icon/Glyph.component.tsx";
import { Popover } from "../Popover/Popover.component.tsx";

export function InfoTip({ label, children, className }: { label: string; children: ReactNode; className?: string }) {
  const anchor = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        ref={anchor}
        type="button"
        aria-label={label}
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className={cn(
          "inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-full text-muted hover:text-ink focus-visible:text-ink",
          className,
        )}
      >
        <Glyph name="info" size={14} />
      </button>
      <Popover open={open} onClose={() => setOpen(false)} anchor={anchor} label={label} width={280}>
        <p className="m-0 text-xs leading-relaxed text-ink-2">{children}</p>
      </Popover>
    </>
  );
}
