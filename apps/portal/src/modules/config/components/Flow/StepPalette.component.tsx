import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";

import { Glyph, cn, spring } from "@flexdeck/ui";

import type { ActionDefinition, Block } from "../../../../services/api/api.ts";
import { groupActions } from "../../lib/actionPicker.lib.ts";
import { actionBlock, emptyBlock, type BlockKind } from "../../lib/flow.lib.ts";
import { StepTag } from "./StepTag.component.tsx";

interface Option {
  key: string;
  group: string;
  title: string;
  detail: string;
  kind: BlockKind;
  make: () => Block;
}

const LOGIC: readonly { kind: BlockKind; title: string; detail: string }[] = [
  { kind: "if", title: "If…", detail: "Do something only when a live value says so" },
  { kind: "wait", title: "Wait", detail: "Pause before the next step" },
  { kind: "repeat", title: "Repeat", detail: "Run some steps a number of times" },
  { kind: "setVariable", title: "Remember a value", detail: "Change a value a plugin lets you set, such as volume or brightness" },
];

/**
 * Add a step: one search over every action on this desktop and the logic
 * steps, grouped by plugin, driven from the keyboard if wanted — type, arrows,
 * Enter. Escape or a click outside dismisses it with nothing chosen.
 */
export function StepPalette({
  open,
  catalogue,
  onPick,
  onClose,
}: {
  open: boolean;
  catalogue: readonly ActionDefinition[];
  onPick: (block: Block) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const search = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);

  const options = useMemo(() => {
    const words = query.trim().toLowerCase();
    const logic: Option[] = LOGIC.filter(
      (entry) => !words || `${entry.title} ${entry.detail} logic`.toLowerCase().includes(words),
    ).map((entry) => ({ key: entry.kind, group: "Logic", title: entry.title, detail: entry.detail, kind: entry.kind, make: () => emptyBlock(entry.kind) }));
    const actions: Option[] = groupActions(catalogue, query).flatMap((group) =>
      group.actions.map((entry) => ({
        key: `${entry.plugin}:${entry.action}`,
        group: group.name,
        title: entry.description || entry.action,
        detail: entry.action,
        kind: "doAction" as const,
        make: () => actionBlock(entry),
      })),
    );
    // Actions first when searching: that is usually what a search is for.
    return words ? [...actions, ...logic] : [...logic, ...actions];
  }, [catalogue, query]);

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActive(0);
    requestAnimationFrame(() => search.current?.focus());
  }, [open]);

  useEffect(() => {
    // `nearest` would tuck the row under the sticky header; leave room for it.
    const row = list.current?.querySelector<HTMLElement>(`[data-index="${active}"]`);
    const box = list.current;
    if (!row || !box) return;
    const header = 40;
    if (row.offsetTop - header < box.scrollTop) box.scrollTop = row.offsetTop - header;
    else if (row.offsetTop + row.offsetHeight > box.scrollTop + box.clientHeight) box.scrollTop = row.offsetTop + row.offsetHeight - box.clientHeight;
  }, [active]);

  const choose = (option: Option | undefined) => {
    if (!option) return;
    onPick(option.make());
  };

  const onKey = (event: KeyboardEvent) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive((index) => Math.min(options.length - 1, index + 1));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((index) => Math.max(0, index - 1));
    } else if (event.key === "Enter") {
      event.preventDefault();
      choose(options[active]);
    } else if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  };

  // Consecutive options of one group, each keeping its place in the flat list the arrows move through.
  const groups: { name: string; items: { option: Option; index: number }[] }[] = [];
  options.forEach((option, index) => {
    const last = groups[groups.length - 1];
    if (last && last.name === option.group) last.items.push({ option, index });
    else groups.push({ name: option.group, items: [{ option, index }] });
  });

  return (
    <AnimatePresence>
      {open ? (
        <motion.div
          key="scrim"
          className="fixed inset-0 z-50 flex items-start justify-center bg-scrim px-3 pt-[12vh] backdrop-blur-[4px]"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.18 }}
          onMouseDown={onClose}
        >
          <motion.div
            role="dialog"
            aria-label="Add a step"
            className="w-full max-w-[600px] overflow-hidden rounded-[20px] border border-line-strong bg-plate shadow-[0_40px_100px_var(--fd-key-cast)]"
            initial={{ opacity: 0, y: -14, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -8, scale: 0.98 }}
            transition={spring.panel}
            onMouseDown={(event) => event.stopPropagation()}
          >
            <div className="flex items-center gap-3 border-b border-line px-4 py-3">
              <Glyph name="search" size={18} className="text-muted" />
              <input
                ref={search}
                value={query}
                onChange={(event) => {
                  setQuery(event.target.value);
                  setActive(0);
                }}
                onKeyDown={onKey}
                role="combobox"
                aria-expanded="true"
                aria-label="Search steps"
                placeholder="Search actions, plugins, logic…"
                className="h-8 flex-1 bg-transparent text-[15px] text-ink outline-none placeholder:text-faint"
              />
              <kbd className="rounded-md border border-line px-1.5 py-0.5 font-mono text-[10px] text-faint">ESC</kbd>
            </div>
            <div ref={list} role="listbox" className="relative max-h-[min(420px,60vh)] overflow-y-auto px-2 pb-2">
              {options.length === 0 ? <p className="m-0 px-3 py-6 text-center text-[13px] text-muted">Nothing matches “{query}”.</p> : null}
              {groups.map((group) => (
                <section key={group.name} aria-label={group.name}>
                  {/* Sticky, so the plugin a row belongs to is always in view while scrolling its actions. */}
                  <div className="lbl sticky top-0 z-10 -mx-2 border-b border-line/60 bg-plate/95 px-5 pt-3 pb-2 backdrop-blur-sm">{group.name}</div>
                  <div className="flex flex-col gap-0.5 pt-1">
                    {group.items.map(({ option, index }) => (
                      <button
                        key={option.key}
                        type="button"
                        role="option"
                        aria-selected={index === active}
                        data-index={index}
                        onMouseMove={() => setActive(index)}
                        onClick={() => choose(option)}
                        className={cn(
                          "flex w-full cursor-pointer items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors",
                          index === active ? "bg-plate-2" : "hover:bg-plate-2/60",
                        )}
                      >
                        <StepTag kind={option.kind} plugin={option.kind === "doAction" ? option.group : undefined} />
                        <span className="min-w-0 flex-1">
                          <span className="block truncate text-sm">{option.title}</span>
                          <span className="block truncate font-mono text-[11px] text-muted">{option.detail}</span>
                        </span>
                        {index === active ? <Glyph name="arrowRight" size={14} className="text-signal" /> : null}
                      </button>
                    ))}
                  </div>
                </section>
              ))}
            </div>
          </motion.div>
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}
