import { useMemo, useState, type ReactNode } from "react";

import { Glyph, Input, Key, cn } from "@flexdeck/ui";

import { childPath, containers, entries, hintOf, isContainer, openTo, search, sizeOf, type Json } from "./jsonTree.lib.ts";

export interface JsonTreeProps {
  value: Json;
  /** How many levels are open at first. */
  depth?: number;
  className?: string;
}

/**
 * A JSON document to read, not a wall of text: objects and arrays fold, a
 * folded object says what it is (`widget-11 · Intermission`), and a search
 * opens the way down to every match. Copy takes the whole document.
 */
export function JsonTree({ value, depth = 2, className }: JsonTreeProps) {
  const [open, setOpen] = useState<Set<string>>(() => openTo(value, depth));
  // Folded by hand while a search holds them open; forgotten with the search.
  const [folded, setFolded] = useState<Set<string>>(() => new Set());
  const [query, setQuery] = useState("");
  const [copied, setCopied] = useState(false);

  const found = useMemo(() => search(value, query), [value, query]);
  const shown = useMemo(() => {
    const next = new Set([...open, ...found.open]);
    folded.forEach((path) => next.delete(path));
    return next;
  }, [open, found, folded]);

  const toggle = (path: string) => {
    const closing = shown.has(path);
    setOpen((current) => {
      const next = new Set(current);
      if (closing) next.delete(path);
      else next.add(path);
      return next;
    });
    setFolded((current) => {
      const next = new Set(current);
      if (closing) next.add(path);
      else next.delete(path);
      return next;
    });
  };

  const find = (text: string) => {
    setQuery(text);
    setFolded(new Set());
  };

  const copy = () => {
    void navigator.clipboard?.writeText(JSON.stringify(value, null, 2)).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    });
  };

  return (
    <div className={cn("flex flex-col gap-3", className)}>
      <div className="flex flex-wrap items-center gap-2">
        <Input
          value={query}
          onChange={(event) => find(event.target.value)}
          placeholder="Find a key or value"
          spellCheck={false}
          aria-label="Find in the document"
          className="h-8 min-w-48 flex-1 font-mono text-[12px]"
        />
        {query.trim() ? (
          <span className="font-mono text-[10px] tracking-[0.14em] text-muted uppercase">
            {found.hits.size === 1 ? "1 match" : `${found.hits.size} matches`}
          </span>
        ) : null}
        <Key
          size="sm"
          onClick={() => {
            setFolded(new Set());
            setOpen(new Set(containers(value).map(([path]) => path)));
          }}
        >
          Expand all
        </Key>
        <Key
          size="sm"
          onClick={() => {
            find("");
            setOpen(new Set([""]));
          }}
        >
          Collapse all
        </Key>
        <Key size="sm" onClick={copy} aria-label="Copy the document">
          <Glyph name={copied ? "check" : "copy"} size={13} />
          {copied ? "Copied" : "Copy"}
        </Key>
      </div>
      <div className="max-h-[70vh] overflow-auto rounded-xl border border-line bg-well p-3 font-mono text-[12px] leading-[1.7] shadow-well">
        <Node name={null} value={value} path="" open={shown} hits={found.hits} onToggle={toggle} last />
      </div>
    </div>
  );
}

interface NodeProps {
  /** The key or index this sits under; null for the root. */
  name: string | number | null;
  value: Json;
  path: string;
  open: Set<string>;
  hits: Set<string>;
  onToggle: (path: string) => void;
  /** No trailing comma. */
  last: boolean;
}

function Node({ name, value, path, open, hits, onToggle, last }: NodeProps) {
  const comma = last ? null : <span className="text-faint">,</span>;
  const label =
    name === null ? null : (
      <>
        <span className={cn(typeof name === "number" ? "text-faint" : "text-ink-2", hits.has(path) && "rounded-sm bg-signal/25 text-ink")}>
          {typeof name === "number" ? name : `"${name}"`}
        </span>
        <span className="text-faint">: </span>
      </>
    );

  if (!isContainer(value)) {
    return (
      <div className="pl-[1.4em] break-all whitespace-pre-wrap">
        {label}
        <Scalar value={value} hit={hits.has(path)} />
        {comma}
      </div>
    );
  }

  const [opening, closing] = Array.isArray(value) ? ["[", "]"] : ["{", "}"];
  const items = entries(value);
  const isOpen = open.has(path);
  const empty = items.length === 0;
  const hint = hintOf(value);

  const head: ReactNode = (
    <button
      type="button"
      onClick={() => !empty && onToggle(path)}
      aria-expanded={empty ? undefined : isOpen}
      className={cn("flex w-full items-start gap-[0.4em] rounded-sm text-left", !empty && "cursor-pointer hover:bg-plate")}
    >
      <span className="flex h-[1.7em] w-[1em] shrink-0 items-center justify-center text-faint">
        {empty ? null : <Glyph name="chevronDown" size={11} className={cn("transition-transform", !isOpen && "-rotate-90")} />}
      </span>
      <span className="min-w-0 break-all">
        {label}
        <span className="text-faint">{opening}</span>
        {empty ? (
          <span className="text-faint">{closing}</span>
        ) : isOpen ? null : (
          <>
            <span className="px-[0.4em] text-muted">{hint ?? sizeOf(value)}</span>
            {hint ? <span className="pr-[0.4em] text-faint">({sizeOf(value)})</span> : null}
            <span className="text-faint">{closing}</span>
          </>
        )}
        {empty || !isOpen ? comma : null}
      </span>
    </button>
  );

  if (empty || !isOpen) return head;

  return (
    <div>
      {head}
      <div className="ml-[0.45em] border-l border-line pl-[0.5em]">
        {items.map(([key, child], index) => (
          <Node
            key={key}
            name={key}
            value={child}
            path={childPath(path, key)}
            open={open}
            hits={hits}
            onToggle={onToggle}
            last={index === items.length - 1}
          />
        ))}
      </div>
      <div className="pl-[1.4em]">
        <span className="text-faint">{closing}</span>
        {comma}
      </div>
    </div>
  );
}

function Scalar({ value, hit }: { value: null | boolean | number | string; hit: boolean }) {
  const tone =
    typeof value === "string" ? "text-ok" : typeof value === "number" ? "text-signal" : "text-warn";
  return (
    <span className={cn(tone, hit && "rounded-sm bg-signal/25")}>
      {typeof value === "string" ? JSON.stringify(value) : String(value)}
    </span>
  );
}
