/**
 * Completing a variable name one dotted level at a time: `obs.` offers
 * `obs.input.`, `obs.output.`, … and only the last level offers the value.
 */

export interface Completion {
  /** The name up to and including this level; a branch ends with a dot. */
  text: string;
  leaf: boolean;
  /** How many values sit under a branch. */
  count: number;
  description?: string;
}

/** Where in a field a name is being typed, and what is typed so far. */
export interface Slot {
  start: number;
  end: number;
  query: string;
  /** In a template, a name goes between braces. */
  braces: boolean;
  /** The opening braces are not there yet. */
  open: boolean;
}

const NAME_CHARS = /^[^\s{}|]*$/;

/** The next level under what is typed, branches before values, each alphabetical. */
export function completions(
  variables: readonly { name: string; description?: string }[],
  typed: string,
  limit = 60,
): Completion[] {
  const dot = typed.lastIndexOf(".");
  const base = typed.slice(0, dot + 1);
  const partial = typed.slice(dot + 1).toLowerCase();
  const branches = new Map<string, Completion>();
  const leaves = new Map<string, Completion>();
  for (const variable of variables) {
    if (!variable.name.startsWith(base)) continue;
    const rest = variable.name.slice(base.length);
    const next = rest.indexOf(".");
    const segment = next === -1 ? rest : rest.slice(0, next);
    if (!segment || !segment.toLowerCase().startsWith(partial)) continue;
    if (next === -1) {
      leaves.set(variable.name, { text: variable.name, leaf: true, count: 1, description: variable.description });
    } else {
      const text = `${base}${segment}.`;
      const branch = branches.get(text);
      if (branch) branch.count += 1;
      else branches.set(text, { text, leaf: false, count: 1 });
    }
  }
  const byText = (a: Completion, b: Completion) => a.text.localeCompare(b.text);
  return [...[...branches.values()].sort(byText), ...[...leaves.values()].sort(byText)].slice(0, limit);
}

/** A bare name field: all of it is the name, braces or not. */
export function nameSlot(text: string): Slot {
  const query = text.replace(/^\{\{\s*/, "").replace(/\s*\}\}$/, "");
  return { start: 0, end: text.length, query, braces: false, open: false };
}

/**
 * A template: the name inside the `{{` the caret sits in. An empty field
 * offers names too, and adds the braces when one is picked.
 */
export function templateSlot(text: string, caret: number): Slot | null {
  if (!text) return { start: 0, end: 0, query: "", braces: true, open: true };
  const before = text.slice(0, caret);
  const opened = before.lastIndexOf("{{");
  if (opened === -1 || before.indexOf("}}", opened) !== -1) return null;
  const inner = before.slice(opened + 2);
  const start = opened + 2 + (inner.length - inner.trimStart().length);
  const query = before.slice(start);
  if (!NAME_CHARS.test(query)) return null;
  const tail = /^[^\s{}|]*/.exec(text.slice(caret))![0];
  return { start, end: caret + tail.length, query, braces: true, open: false };
}

/** The field with a completion put in its slot, and where the caret goes. */
export function applyCompletion(text: string, slot: Slot, completion: Completion): { text: string; caret: number } {
  const head = text.slice(0, slot.start) + (slot.open ? "{{" : "") + completion.text;
  let tail = text.slice(slot.end);
  if (completion.leaf && slot.braces && !/^\s*\}\}/.test(tail)) tail = `}}${tail}`;
  const caret = head.length + (completion.leaf && slot.braces ? (/^\s*\}\}/.exec(tail)?.[0].length ?? 0) : 0);
  return { text: head + tail, caret };
}

/** A `{{` with no `}}` after it: half a name, which the desktop would refuse. */
export function unclosedBraces(text: string): boolean {
  return /\{\{(?![\s\S]*\}\})/.test(text);
}
