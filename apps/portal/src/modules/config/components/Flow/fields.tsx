import { useEffect, useId, useState, type InputHTMLAttributes } from "react";

import { cn } from "@flexdeck/ui";

import type { VariableChoice } from "../../../../services/api/api.ts";

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
 * A value field: literal text, or a live value written `{{name}}`. Offers the
 * desktop's variables as suggestions, so a value is picked, not remembered.
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
  const list = useId();
  const offered = kinds ? variables.filter((variable) => kinds.includes(variable.kind)) : variables;
  return (
    <>
      <DraftInput
        aria-label={label}
        value={value}
        onCommit={onCommit}
        list={list}
        placeholder={placeholder}
        spellCheck={false}
        className={cn(/\{\{/.test(value) && "border-signal-line bg-signal-soft font-mono text-signal-hi", className)}
      />
      <datalist id={list}>
        {offered.map((variable) => (
          <option key={variable.name} value={`{{${variable.name}}}`}>
            {variable.description}
          </option>
        ))}
      </datalist>
    </>
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
  const list = useId();
  const offered = kinds ? variables.filter((variable) => kinds.includes(variable.kind)) : variables;
  return (
    <>
      <DraftInput
        aria-label={label}
        value={value}
        onCommit={(next) => onCommit(next.replace(/^\{\{\s*|\s*\}\}$/g, "").trim())}
        list={list}
        placeholder="variable"
        spellCheck={false}
        className={cn("font-mono text-[12px]", className)}
      />
      <datalist id={list}>
        {offered.map((variable) => (
          <option key={variable.name} value={variable.name}>
            {variable.description}
          </option>
        ))}
      </datalist>
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
