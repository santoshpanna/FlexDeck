/**
 * The pure half of the JSON viewer: paths, folding, hints and search, kept out
 * of the component so they test under `node --test`.
 *
 * A path is the chain of keys and indexes from the root, joined with `/`
 * (`""` is the root itself). Keys with a `/` in them are escaped, so two paths
 * never collide.
 */

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

export function isContainer(value: Json): value is Json[] | { [key: string]: Json } {
  return value !== null && typeof value === "object";
}

/** One step deeper. */
export function childPath(path: string, key: string | number): string {
  const part = String(key).replace(/~/g, "~0").replace(/\//g, "~1");
  return path === "" ? part : `${path}/${part}`;
}

/** Every object and array in `value`, with how deep each sits (the root is 0). */
export function containers(value: Json, path = "", depth = 0, out: [string, number][] = []): [string, number][] {
  if (!isContainer(value)) return out;
  out.push([path, depth]);
  for (const [key, child] of entries(value)) containers(child, childPath(path, key), depth + 1, out);
  return out;
}

/** The containers open at first: everything shallower than `depth`. */
export function openTo(value: Json, depth: number): Set<string> {
  return new Set(containers(value).filter(([, d]) => d < depth).map(([path]) => path));
}

export function entries(value: Json[] | { [key: string]: Json }): [string | number, Json][] {
  return Array.isArray(value) ? value.map((item, index) => [index, item]) : Object.entries(value);
}

/** What a folded container says about itself: its size. */
export function sizeOf(value: Json[] | { [key: string]: Json }): string {
  const count = Array.isArray(value) ? value.length : Object.keys(value).length;
  if (Array.isArray(value)) return count === 1 ? "1 item" : `${count} items`;
  return count === 1 ? "1 key" : `${count} keys`;
}

/**
 * A word or two that names a folded object — its `id`, and a `name` or
 * `label` if it has one — so a list of widgets reads as widgets, not as
 * twenty identical `{…}`.
 */
export function hintOf(value: Json): string | null {
  if (!isContainer(value) || Array.isArray(value)) return null;
  const text = (candidate: Json | undefined) => (typeof candidate === "string" && candidate.trim() ? candidate.trim() : null);
  const props = value.props;
  const named =
    text(value.name) ?? text(value.label) ?? text(value.title) ?? (isContainer(props ?? null) && !Array.isArray(props) ? text((props as Record<string, Json>).label) : null);
  const id = text(value.id);
  const parts = [id, named].filter((part): part is string => part !== null && part !== "");
  return parts.length ? [...new Set(parts)].join(" · ") : null;
}

/** Whether a key or a plain value contains `query`, ignoring case. */
export function matches(text: string, query: string): boolean {
  return query !== "" && text.toLowerCase().includes(query.toLowerCase());
}

/**
 * Where `query` turns up — in a key or in a plain value — and every container
 * that has to be open to see it.
 */
export function search(value: Json, query: string): { hits: Set<string>; open: Set<string> } {
  const hits = new Set<string>();
  const open = new Set<string>();
  const trimmed = query.trim();
  if (!trimmed) return { hits, open };

  const walk = (node: Json, path: string, ancestors: string[]): void => {
    if (!isContainer(node)) {
      if (matches(node === null ? "null" : String(node), trimmed)) {
        hits.add(path);
        ancestors.forEach((ancestor) => open.add(ancestor));
      }
      return;
    }
    for (const [key, child] of entries(node)) {
      const here = childPath(path, key);
      if (!Array.isArray(node) && matches(String(key), trimmed)) {
        hits.add(here);
        [...ancestors, path].forEach((ancestor) => open.add(ancestor));
      }
      walk(child, here, [...ancestors, path]);
    }
  };
  walk(value, "", []);
  return { hits, open };
}
