import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type InputHTMLAttributes } from "react";
import { createPortal } from "react-dom";

import { cn } from "@flexdeck/ui";

import type { VariableChoice } from "../../../../services/api/api.ts";
import { applyCompletion, completions, expressionSlot, nameSlot, templateSlot, unclosedBraces, type Completion, type Slot } from "../../lib/suggest.lib.ts";

export const control =
  "h-9 w-full min-w-0 rounded-[9px] border border-line bg-well px-2.5 text-[13px] text-ink shadow-well placeholder:text-faint focus:outline-2 focus:outline-signal focus:outline-offset-1";

/**
 * A text box that keeps its own draft and commits on blur or Enter. Every
 * commit is a revision on the desktop, so one per keystroke would bury the undo
 * history — and would re-render the card under the caret.
 */
export function DraftInput({
  value,
  onCommit,
  className,
  ...rest
}: Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange"> & {
  value: string;
  onCommit: (value: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => {
    if (draft !== value) onCommit(draft);
  };
  return (
    <input
      {...rest}
      value={draft}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter") commit();
        if (event.key === "Escape") setDraft(value);
      }}
      className={cn(control, className)}
    />
  );
}

/**
 * A value field: literal text, or a live value written `{{name}}`. Inside the
 * braces it offers the desktop's variables a level at a time, so a value is
 * picked, not remembered.
 */
export function ValueInput({
  value,
  onCommit,
  variables,
  kinds,
  placeholder,
  className,
  label,
}: {
  value: string;
  onCommit: (value: string) => void;
  variables: readonly VariableChoice[];
  kinds?: readonly VariableChoice["kind"][];
  placeholder?: string;
  className?: string;
  label: string;
}) {
  return (
    <SuggestInput
      label={label}
      value={value}
      onCommit={onCommit}
      variables={variables}
      kinds={kinds}
      slotAt={templateSlot}
      holdWhile={unclosedBraces}
      placeholder={placeholder}
      className={cn(/\{\{/.test(value) && "border-signal-line bg-signal-soft font-mono text-signal-hi", className)}
    />
  );
}

export function ExpressionInput({
  value,
  onCommit,
  variables,
  placeholder,
  className,
  label,
}: {
  value: string;
  onCommit: (value: string) => void;
  variables: readonly VariableChoice[];
  placeholder?: string;
  className?: string;
  label: string;
}) {
  return (
    <SuggestInput
      label={label}
      value={value}
      onCommit={onCommit}
      variables={variables}
      slotAt={expressionSlot}
      placeholder={placeholder}
      className={cn("font-mono text-[12px]", className)}
    />
  );
}

/** Choose a variable by name, from what the plugins publish — or type one that is not reported yet. */
export function VariableInput({
  value,
  onCommit,
  variables,
  kinds,
  label,
  className,
}: {
  value: string;
  onCommit: (value: string) => void;
  variables: readonly VariableChoice[];
  kinds?: readonly VariableChoice["kind"][];
  label: string;
  className?: string;
}) {
  return (
    <SuggestInput
      label={label}
      value={value}
      onCommit={(next) => onCommit(next.replace(/^\{\{\s*|\s*\}\}$/g, "").trim())}
      variables={variables}
      kinds={kinds}
      slotAt={nameSlot}
      commitOnLeaf
      placeholder="variable"
      className={cn("font-mono text-[12px]", className)}
    />
  );
}

/**
 * A draft input with the names that fit under it. Picking a branch (`obs.`)
 * puts it in and lists the next level; picking a value finishes the name.
 */
function SuggestInput({
  label,
  value,
  onCommit,
  variables,
  kinds,
  slotAt,
  commitOnLeaf,
  holdWhile,
  placeholder,
  className,
}: {
  label: string;
  value: string;
  onCommit: (value: string) => void;
  variables: readonly VariableChoice[];
  kinds?: readonly VariableChoice["kind"][];
  slotAt: (text: string, caret: number) => Slot | null;
  commitOnLeaf?: boolean;
  /** Not saved while this holds; the draft stays, marked, until it is fixed. */
  holdWhile?: (text: string) => boolean;
  placeholder?: string;
  className?: string;
}) {
  const input = useRef<HTMLInputElement>(null);
  const listId = useId();
  const [draft, setDraft] = useState(value);
  const [caret, setCaret] = useState(0);
  const [focused, setFocused] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const [active, setActive] = useState(0);
  // Until the user types or moves through the list, Tab and Enter keep their
  // usual meaning: a list that opened on focus must not fill the field in.
  const [engaged, setEngaged] = useState(false);
  const [place, setPlace] = useState<{ top: number; left: number; width: number } | null>(null);
  const pendingCaret = useRef<number | null>(null);
  const committed = useRef(value);
  useEffect(() => {
    setDraft(value);
    committed.current = value;
  }, [value]);

  const offered = useMemo(() => (kinds ? variables.filter((variable) => kinds.includes(variable.kind)) : variables), [variables, kinds]);
  const slot = focused && !dismissed ? slotAt(draft, caret) : null;
  const query = slot?.query ?? null;
  const items = useMemo(() => (query === null ? [] : completions(offered, query)), [offered, query]);
  const open = items.length > 0;

  useEffect(() => setActive(0), [query]);

  useEffect(() => {
    if (open) document.getElementById(`${listId}-${active}`)?.scrollIntoView({ block: "nearest" });
  }, [open, active, listId]);

  useLayoutEffect(() => {
    const at = pendingCaret.current;
    if (at === null || !input.current) return;
    input.current.setSelectionRange(at, at);
    setCaret(at);
    pendingCaret.current = null;
  }, [draft]);

  useLayoutEffect(() => {
    if (!open) return;
    const measure = () => {
      const at = input.current?.getBoundingClientRect();
      if (at) setPlace({ top: at.bottom + 4, left: at.left, width: Math.max(at.width, 260) });
    };
    measure();
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, true);
    return () => {
      window.removeEventListener("resize", measure);
      window.removeEventListener("scroll", measure, true);
    };
  }, [open]);

  const held = holdWhile?.(draft) ?? false;
  const commit = (next = draft) => {
    if (next === committed.current || holdWhile?.(next)) return;
    committed.current = next;
    onCommit(next);
  };
  const pick = (completion: Completion) => {
    if (!slot) return;
    const applied = applyCompletion(draft, slot, completion);
    if (applied.text === draft) {
      input.current?.setSelectionRange(applied.caret, applied.caret);
      setCaret(applied.caret);
    } else {
      pendingCaret.current = applied.caret;
      setDraft(applied.text);
    }
    if (completion.leaf) {
      setDismissed(true);
      if (commitOnLeaf) commit(applied.text);
    }
  };
  const track = () => setCaret(input.current?.selectionStart ?? 0);

  return (
    <>
      <input
        ref={input}
        aria-label={label}
        role="combobox"
        aria-expanded={open}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={open ? `${listId}-${active}` : undefined}
        autoComplete="off"
        spellCheck={false}
        placeholder={placeholder}
        value={draft}
        onChange={(event) => {
          setDraft(event.target.value);
          setCaret(event.target.selectionStart ?? 0);
          setDismissed(false);
          setEngaged(true);
        }}
        onSelect={track}
        onFocus={() => {
          setFocused(true);
          setEngaged(false);
          track();
        }}
        onBlur={() => {
          setFocused(false);
          setDismissed(false);
          commit();
        }}
        onKeyDown={(event) => {
          if (open && event.key === "ArrowDown") {
            event.preventDefault();
            setEngaged(true);
            setActive((index) => (index + 1) % items.length);
          } else if (open && event.key === "ArrowUp") {
            event.preventDefault();
            setEngaged(true);
            setActive((index) => (index - 1 + items.length) % items.length);
          } else if (open && engaged && (event.key === "Enter" || event.key === "Tab") && items[active]) {
            event.preventDefault();
            pick(items[active]);
          } else if (open && event.key === "Escape") {
            event.stopPropagation();
            setDismissed(true);
          } else if (event.key === "Enter") commit();
          else if (event.key === "Escape") setDraft(value);
        }}
        aria-invalid={held ? true : undefined}
        title={held ? "Close the {{ … }} to save this" : undefined}
        className={cn(control, className, held && "border-warn")}
      />
      {open && place
        ? createPortal(
            <ul
              id={listId}
              role="listbox"
              aria-label={`${label} suggestions`}
              className="fixed z-50 m-0 max-h-64 list-none overflow-y-auto rounded-[11px] border border-line bg-plate p-1 shadow-sheet"
              style={place}
              // Keeps focus in the input, so a click does not blur and commit half a name.
              onMouseDown={(event) => event.preventDefault()}
            >
              {items.map((item, index) => (
                <li
                  key={item.text}
                  id={`${listId}-${index}`}
                  role="option"
                  aria-selected={engaged && index === active}
                  onMouseEnter={() => {
                    setActive(index);
                    setEngaged(true);
                  }}
                  onClick={() => pick(item)}
                  className={cn("flex cursor-pointer items-baseline gap-2 rounded-[7px] px-2 py-1.5", engaged && index === active && "bg-well")}
                >
                  <span className="shrink-0 font-mono text-[12px] text-ink">{item.text}</span>
                  <span className="min-w-0 truncate text-[11px] text-faint">{item.leaf ? item.description : item.count}</span>
                </li>
              ))}
            </ul>,
            document.body,
          )
        : null}
    </>
  );
}

/** A label-sized chip naming a live value, the way values read everywhere in the editor. */
export function ValueChip({ name, className }: { name: string; className?: string }) {
  return (
    <span
      className={cn(
        "inline-flex h-6 items-center rounded-[7px] border border-signal-line bg-signal-soft px-2 font-mono text-[11px] text-signal-hi",
        className,
      )}
    >
      {name}
    </span>
  );
}

/** Text with its `{{values}}` drawn as chips: how a template reads back. */
export function TemplatePreview({ text, className }: { text: string; className?: string }) {
  const parts = text.split(/(\{\{\s*[^{}]+?\s*\}\})/g).filter(Boolean);
  return (
    <span className={cn("inline-flex flex-wrap items-center gap-1 text-[13px]", className)}>
      {parts.map((part, index) => {
        const match = /^\{\{\s*([^{}]+?)\s*\}\}$/.exec(part);
        return match ? <ValueChip key={index} name={match[1]!} /> : <span key={index}>{part}</span>;
      })}
    </span>
  );
}
