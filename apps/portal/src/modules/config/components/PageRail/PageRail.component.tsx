import { Reorder, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";

import { Glyph, HoldToConfirm, Key, cn, spring } from "@flexdeck/ui";

import type { WirePage } from "../../../../services/api/api.ts";
import { pageNameOf } from "../../lib/config.lib.ts";
import { control } from "../Flow/fields.tsx";

export interface PageKeysProps {
  pages: readonly WirePage[];
  currentId: string | null;
  onSelect: (pageId: string) => void;
  onAdd: () => void;
  /** Move a page by `delta` places — what dropping a dragged page key does. */
  onMove: (pageId: string, delta: number) => void;
}

/**
 * The pages as a column of keycaps beside the deck: the current one lit, a
 * blank one at the bottom to add another. Drag a page key up or down to put
 * the page somewhere else; the others make room as it passes.
 */
export function PageKeys({ pages, currentId, onSelect, onAdd, onMove }: PageKeysProps) {
  const [order, setOrder] = useState(() => pages.map((page) => page.id));
  const dragging = useRef<string | null>(null);
  useEffect(() => {
    if (!dragging.current) setOrder(pages.map((page) => page.id));
  }, [pages]);
  const byId = new Map(pages.map((page) => [page.id, page] as const));

  return (
    <nav aria-label="Pages" className="flex flex-col items-center gap-2.5">
      <span className="lbl text-[9px]">Pages</span>
      <Reorder.Group axis="y" values={order} onReorder={setOrder} className="m-0 flex list-none flex-col gap-2.5 p-0">
      {order.map((id, index) => {
        const page = byId.get(id);
        if (!page) return null;
        const on = page.id === currentId;
        return (
          <Reorder.Item
            key={page.id}
            value={page.id}
            onDragStart={() => {
              dragging.current = page.id;
            }}
            onDragEnd={() => {
              dragging.current = null;
              const from = pages.findIndex((candidate) => candidate.id === page.id);
              const to = order.indexOf(page.id);
              if (from >= 0 && to >= 0 && from !== to) onMove(page.id, to - from);
            }}
            whileDrag={{ scale: 1.08, zIndex: 10 }}
            className="relative"
          >
          <button
            type="button"
            title={pageNameOf(page)}
            aria-label={pageNameOf(page)}
            aria-current={on ? "page" : undefined}
            onClick={() => onSelect(page.id)}
            className={cn(
              "relative flex size-14 cursor-pointer flex-col items-center justify-center rounded-key border font-mono text-[13px] shadow-key transition-[transform,color] active:translate-y-[2px] active:shadow-key-pressed",
              "border-[var(--fd-key-line)] bg-linear-to-b from-[var(--fd-key-top)] to-[var(--fd-key-bottom)]",
              on ? "text-ink" : "text-muted hover:text-ink",
            )}
          >
            {on ? (
              <motion.span layoutId="page-ring" transition={spring.panel} className="absolute -inset-[3px] rounded-[17px] border-2 border-signal" />
            ) : null}
            {String(index + 1).padStart(2, "0")}
            <span className="max-w-[48px] truncate font-sans text-[9px] text-faint">{pageNameOf(page)}</span>
          </button>
          </Reorder.Item>
        );
      })}
      </Reorder.Group>
      <button
        type="button"
        onClick={onAdd}
        aria-label="Add a page"
        title="Add a page"
        className="flex size-14 cursor-pointer items-center justify-center rounded-key border border-dashed border-line-strong text-muted transition-colors hover:border-signal hover:text-signal"
      >
        <Glyph name="plus" />
      </button>
    </nav>
  );
}

export interface PageRailProps {
  pages: readonly WirePage[];
  currentId: string | null;
  onAdd: () => void;
  onRename: (pageId: string, name: string) => void;
  onMove: (pageId: string, delta: number) => void;
  onDuplicate: (pageId: string) => void;
  onRemove: (pageId: string) => void;
}

/**
 * The current page's own controls: its name, and moving, duplicating and
 * deleting it. The rename box commits on blur or Enter — every commit is a
 * revision, and one per keystroke would bury the undo history.
 */
export function PageRail({ pages, currentId, onRename, onDuplicate, onRemove }: PageRailProps) {
  const index = pages.findIndex((page) => page.id === currentId);
  const current = index >= 0 ? pages[index] : null;
  const [draft, setDraft] = useState(current?.name ?? "");
  useEffect(() => setDraft(current?.name ?? ""), [currentId]);
  if (!current) return null;

  return (
    <div className="flex flex-col gap-3">
      <input
        value={draft}
        placeholder="Page name"
        aria-label="Page name"
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => onRename(current.id, draft)}
        onKeyDown={(event) => event.key === "Enter" && onRename(current.id, draft)}
        className={cn(control, "h-10 text-sm")}
      />
      <div className="flex flex-wrap gap-2">
        <Key size="sm" onClick={() => onDuplicate(current.id)}>
          <Glyph name="copy" size={13} /> Duplicate
        </Key>
        <HoldToConfirm size="sm" tone="danger" disabled={pages.length <= 1} onConfirm={() => onRemove(current.id)} className="ml-auto">
          Delete page
        </HoldToConfirm>
      </div>
    </div>
  );
}
