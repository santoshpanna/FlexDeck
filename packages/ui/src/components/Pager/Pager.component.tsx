import { cn } from "../../lib/cn.ts";
import { Glyph } from "../Icon/Glyph.component.tsx";
import { Key } from "../Key/Key.component.tsx";

export interface PagerProps {
  page: number;
  pages: number;
  from: number;
  to: number;
  total: number;
  onPage: (page: number) => void;
  className?: string;
}

/** Previous / next for a long list, with where you are in it. Renders nothing for one page. */
export function Pager({ page, pages, from, to, total, onPage, className }: PagerProps) {
  if (pages <= 1) return null;
  return (
    <nav aria-label="Pages" className={cn("flex items-center justify-between gap-2", className)}>
      <Key size="icon" aria-label="Previous page" disabled={page === 0} onClick={() => onPage(page - 1)}>
        <Glyph name="chevronLeft" size={16} />
      </Key>
      <span className="font-mono text-[10px] tracking-[0.14em] text-muted">
        {from}–{to} OF {total}
      </span>
      <Key size="icon" aria-label="Next page" disabled={page >= pages - 1} onClick={() => onPage(page + 1)}>
        <Glyph name="chevronRight" size={16} />
      </Key>
    </nav>
  );
}
