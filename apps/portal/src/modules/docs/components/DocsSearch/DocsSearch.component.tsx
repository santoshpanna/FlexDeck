import { useEffect, useId, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

import { Input, cn } from "@flexdeck/ui";

import { type Doc, searchDocs } from "../../lib/docs.lib.ts";

/**
 * Search across every doc's title and headings. `/` focuses it from anywhere on
 * the page; arrows move through the hits, Enter opens one, Escape lets go.
 */
export function DocsSearch({ docs }: { docs: readonly Doc[] }) {
  const navigate = useNavigate();
  const input = useRef<HTMLInputElement>(null);
  const listId = useId();
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState(0);
  const hits = searchDocs(docs, query);

  useEffect(() => {
    const focus = (event: KeyboardEvent) => {
      if (event.key !== "/" || event.metaKey || event.ctrlKey || event.altKey) return;
      const target = event.target as HTMLElement | null;
      if (target && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName))) return;
      event.preventDefault();
      input.current?.focus();
    };
    window.addEventListener("keydown", focus);
    return () => window.removeEventListener("keydown", focus);
  }, []);

  const open = (to: string) => {
    setQuery("");
    input.current?.blur();
    navigate(to);
  };

  return (
    <div className="relative">
      <label className="sr-only" htmlFor={`${listId}-input`}>
        Search the docs
      </label>
      <Input
        ref={input}
        id={`${listId}-input`}
        type="search"
        role="combobox"
        aria-expanded={hits.length > 0}
        aria-controls={listId}
        aria-activedescendant={hits.length > 0 ? `${listId}-${cursor}` : undefined}
        autoComplete="off"
        placeholder="Search the docs   /"
        value={query}
        onChange={(event) => {
          setQuery(event.target.value);
          setCursor(0);
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setCursor((at) => Math.min(at + 1, hits.length - 1));
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setCursor((at) => Math.max(at - 1, 0));
          } else if (event.key === "Enter" && hits[cursor]) {
            event.preventDefault();
            open(hits[cursor].to);
          } else if (event.key === "Escape") {
            setQuery("");
            input.current?.blur();
          }
        }}
      />
      {query.trim() ? (
        <ul id={listId} role="listbox" className="absolute top-full right-0 left-0 z-30 m-0 mt-2 flex list-none flex-col overflow-hidden rounded-[14px] border border-line bg-plate p-1 shadow-plate">
          {hits.length === 0 ? <li className="px-3 py-2.5 text-sm text-muted">No match.</li> : null}
          {hits.map((hit, index) => (
            <li
              key={hit.to}
              id={`${listId}-${index}`}
              role="option"
              aria-selected={index === cursor}
              onMouseDown={(event) => {
                event.preventDefault();
                open(hit.to);
              }}
              onMouseEnter={() => setCursor(index)}
              className={cn("flex cursor-pointer flex-col gap-0.5 rounded-[10px] px-3 py-2", index === cursor && "bg-well")}
            >
              <span className="text-sm text-ink">{hit.heading ?? hit.doc}</span>
              {hit.heading ? <span className="font-mono text-[10px] tracking-[0.14em] text-faint uppercase">{hit.doc}</span> : null}
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
