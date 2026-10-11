import { motion } from "motion/react";

import { cn, spring } from "@flexdeck/ui";

import type { WirePage } from "../../../../services/api/api.ts";
import { pageNameOf } from "../../lib/config.lib.ts";
import { pageDots, pageLabel } from "../../lib/dots.lib.ts";

export interface PageDotsProps {
  pages: readonly WirePage[];
  currentId: string | null;
  onSelect: (pageId: string) => void;
}

/**
 * The deck's pager, in the bezel under the screen where the tablet keeps its
 * own: dots (the current one stretched into a pill), the page's name, and its
 * position. Past a handful of pages it shows a window of dots, and the position
 * is always printed, so no page has to be counted to.
 */
export function PageDots({ pages, currentId, onSelect }: PageDotsProps) {
  if (pages.length === 0) return null;
  const current = Math.max(0, pages.findIndex((page) => page.id === currentId));
  const { indices, gapBefore, gapAfter } = pageDots(pages.length, current);
  const name = pages[current] ? pageNameOf(pages[current]) : "";

  return (
    <div className="flex items-center gap-3 px-1">
      {pages.length > 1 ? (
        <div className="flex items-center gap-1.5" role="group" aria-label="Pages">
          {gapBefore ? <span className="text-[10px] text-deck-muted">…</span> : null}
          {indices.map((index) => {
            const page = pages[index]!;
            const on = index === current;
            return (
              <button
                key={page.id}
                type="button"
                aria-current={on}
                aria-label={pageNameOf(page)}
                title={pageNameOf(page)}
                onClick={() => onSelect(page.id)}
                className={cn("relative h-1.5 cursor-pointer rounded-full transition-[width,background] duration-300", on ? "w-5 bg-deck-ink" : "w-1.5 bg-[#2e2e33] hover:bg-deck-muted")}
              >
                {on ? <motion.span layoutId="dot-pill" transition={spring.panel} className="absolute inset-0 rounded-full bg-deck-ink" /> : null}
              </button>
            );
          })}
          {gapAfter ? <span className="text-[10px] text-deck-muted">…</span> : null}
        </div>
      ) : null}
      <span className="truncate text-[12px] text-deck-ink">{name}</span>
      <span className="ml-auto font-mono text-[10px] tracking-[0.14em] text-deck-muted">{pageLabel(current, pages.length)}</span>
    </div>
  );
}
