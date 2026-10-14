import { CONFIG_SCHEMA, type Block, type DataSource, type WireConfig, type WirePage, type WireWidget } from "../../../services/api/api.ts";
import { withSchemaFor } from "./config.lib.ts";
import { nextPageId, nextWidgetId } from "./editing.lib.ts";

/**
 * A page to share: `.fddeck`, one JSON file. It carries the page, the
 * pictures it shows and the data sources of your own its keys read. Secrets
 * never travel: the deck's config does not hold them (the agent keeps them),
 * and a source's secret settings are left blank.
 *
 * Importing one is reviewed first: whatever it would run, type or reach is
 * listed, and nothing is added until that is agreed to.
 */

export const FDDECK_VERSION = 1;

export interface Bundle {
  fddeck: number;
  /** The config schema the page was written in. */
  schema: number;
  page: WirePage;
  /** Pictures it shows, by sha256, as base64. */
  images?: Record<string, string>;
  /** Data sources of your own its keys read. */
  sources?: DataSource[];
}

/** One thing an imported page would do that deserves a look. */
export interface Concern {
  /** The key's name, or "the page". */
  where: string;
  what: string;
  /** Runs a program, types keys, or reaches this computer: shown in red. */
  severe: boolean;
}

export interface Review {
  name: string;
  keys: number;
  pictures: number;
  concerns: Concern[];
  /** Plugins its keys use that this desktop does not have. */
  missing: string[];
  /** Sources it brings; `renamed` when one of yours already has its id. */
  sources: { id: string; name: string; renamed?: string }[];
}

const SHA = /img:([0-9a-f]{64})/g;

/** Every picture (`img:<sha>`) a page shows, anywhere in it. */
export function picturesOf(page: WirePage): string[] {
  const found = new Set<string>();
  for (const match of JSON.stringify(page).matchAll(SHA)) found.add(match[1]!);
  return [...found].sort();
}

/** The ids of data sources a page's keys read: their own runs and `src.<id>.` in templates. */
export function sourcesOf(page: WirePage): string[] {
  const found = new Set<string>();
  for (const widget of page.widgets ?? []) {
    for (const use of widget.sources ?? []) if (use.source && !use.source.includes("/")) found.add(use.source);
  }
  for (const match of JSON.stringify(page).matchAll(/src\.([a-z][a-z0-9_-]*)\./g)) found.add(match[1]!);
  return [...found].sort();
}

/**
 * Bundle `page`, written in config `schema`, with the sources of `yours` it
 * reads. Pictures are filled in by the caller (`images`), which fetches the
 * bytes.
 */
export function exportPage(page: WirePage | null | undefined, schema: number | undefined, yours: readonly DataSource[]): Bundle | null {
  if (!page) return null;
  const wanted = new Set(sourcesOf(page));
  const sources = yours
    .filter((source) => wanted.has(source.id))
    .map((source) => ({ ...source, args: source.args?.map((arg) => (arg.secret ? { ...arg, default: "" } : arg)) }));
  const bundle: Bundle = { fddeck: FDDECK_VERSION, schema: schema ?? withSchemaFor({ profiles: [{ id: "", pages: [page] }] }).schema!, page: structuredClone(page) };
  if (sources.length > 0) bundle.sources = sources;
  return bundle;
}

/** Read a file's text as a bundle, refusing what is not one. */
export function parseBundle(text: string): Bundle {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new Error("This is not a FlexDeck page: it is not JSON.");
  }
  const bundle = value as Partial<Bundle>;
  if (typeof bundle !== "object" || bundle === null || typeof bundle.fddeck !== "number" || typeof bundle.page !== "object" || bundle.page === null) {
    throw new Error("This is not a FlexDeck page (.fddeck).");
  }
  if (bundle.fddeck > FDDECK_VERSION) throw new Error("This page was made by a newer FlexDeck. Update this one to open it.");
  if ((bundle.schema ?? 0) > CONFIG_SCHEMA) throw new Error("This page needs a newer FlexDeck to show it.");
  return bundle as Bundle;
}

/** Built-ins that act on this computer in ways worth a second look, by plugin and action. */
const SEVERE: Record<string, (action: string) => string | null> = {
  launcher: (action) =>
    action === "run" ? "runs a program" : action === "open_app" ? "opens an app" : action === "open" ? "opens a file or folder" : action === "open_url" ? "opens a web page" : action === "close_app" ? "closes an app" : null,
  core: (action) => (action === "hotkey" ? "presses keys" : action === "text" ? "types text" : null),
};

const BUILT_IN = new Set(["core", "deck", "display", "launcher", "media", "notifications", "sensors", "system", "time", "timer", "vars"]);

function blocksOf(blocks: readonly Block[] | undefined, visit: (block: Extract<Block, { block: "doAction" }>) => void): void {
  for (const block of blocks ?? []) {
    switch (block.block) {
      case "doAction":
        visit(block);
        break;
      case "if":
        blocksOf(block.then, visit);
        for (const branch of block.elseIf ?? []) blocksOf(branch.blocks, visit);
        blocksOf(block.otherwise, visit);
        break;
      case "repeat":
        blocksOf(block.body, visit);
        break;
      default:
        break;
    }
  }
}

function argText(block: Extract<Block, { block: "doAction" }>): string {
  const first = Object.values(block.args ?? {})[0] as { text?: string; number?: number } | undefined;
  const value = first?.text ?? (first?.number !== undefined ? String(first.number) : "");
  return value ? `: ${value.length > 60 ? `${value.slice(0, 57)}…` : value}` : "";
}

/**
 * What importing `bundle` would bring, and what it would do that deserves a
 * look first. `installed` is the plugins this desktop has; `yours` its own
 * data sources.
 */
export function reviewBundle(bundle: Bundle, installed: readonly string[], yours: readonly DataSource[]): Review {
  const page = bundle.page;
  const concerns: Concern[] = [];
  const missing = new Set<string>();
  const have = new Set([...installed, ...BUILT_IN]);
  for (const widget of page.widgets ?? []) {
    const where = widget.props?.label || widget.id;
    for (const flow of widget.flows ?? []) {
      blocksOf(flow.blocks, (block) => {
        if (!have.has(block.plugin)) missing.add(block.plugin);
        const severe = SEVERE[block.plugin]?.(block.action);
        if (severe) concerns.push({ where, what: `${severe}${argText(block)}`, severe: true });
        else if (!BUILT_IN.has(block.plugin)) concerns.push({ where, what: `uses the ${block.plugin} plugin (${block.action})`, severe: false });
      });
    }
    for (const use of widget.sources ?? []) {
      const plugin = use.source.split("/")[0]!;
      if (use.source.includes("/") && !have.has(plugin)) missing.add(plugin);
      // A key may point a source of yours (say, Home Assistant) at an address
      // of its own: what it fetches, and any token it sends, goes there.
      for (const value of Object.values(use.args ?? {})) {
        if (typeof value === "string" && /^[a-z][a-z0-9+.-]*:\/\//i.test(value.trim())) {
          concerns.push({ where, what: `points ${use.source} at ${hostOf(value)}`, severe: true });
        }
      }
    }
  }
  const taken = new Set(yours.map((source) => source.id));
  const sources = (bundle.sources ?? []).map((source) => {
    if (source.kind === "command") {
      concerns.push({ where: "the page", what: `brings a source that runs a program every so often: ${source.command?.program ?? "?"}`, severe: true });
    } else if (source.http?.url) {
      concerns.push({ where: "the page", what: `brings a source that reads ${hostOf(source.http.url)}${source.lan ? ", and may reach your network" : ""}`, severe: Boolean(source.lan) });
    }
    return { id: source.id, name: source.name || source.id, ...(taken.has(source.id) ? { renamed: freeId(source.id, taken) } : {}) };
  });
  return {
    name: page.name || "A page",
    keys: (page.widgets ?? []).length,
    pictures: Object.keys(bundle.images ?? {}).length,
    concerns,
    missing: [...missing].sort(),
    sources,
  };
}

function hostOf(url: string): string {
  const match = /^[a-z]+:\/\/([^/?#{]+)/i.exec(url.trim());
  return match?.[1] ?? url;
}

function freeId(id: string, taken: ReadonlySet<string>): string {
  for (let n = 2; ; n += 1) if (!taken.has(`${id}-${n}`)) return `${id}-${n}`;
}

/**
 * Your sources with `bundle`'s added, one whose id is taken renamed — and
 * the renames, old id to new.
 */
export function importedSources(bundle: Bundle, yours: readonly DataSource[]): { sources: DataSource[]; renames: Map<string, string> } {
  const taken = new Set(yours.map((source) => source.id));
  const renames = new Map<string, string>();
  const added: DataSource[] = [];
  for (const source of bundle.sources ?? []) {
    const id = taken.has(source.id) ? freeId(source.id, taken) : source.id;
    taken.add(id);
    if (id !== source.id) renames.set(source.id, id);
    added.push({ ...source, id });
  }
  return { sources: [...yours, ...added], renames };
}

/**
 * Add `bundle`'s page to `profileId` as a new page, every key with a fresh
 * id, and its sources beside `yours` — one whose id is taken renamed, and
 * the keys that read it following. `pictures` maps each picture's old sha to
 * the `img:` name this desktop kept it under (it re-encodes what it is
 * given). Returns the new config, the new sources (all of them) and the new
 * page's id.
 */
export function importBundle(
  config: WireConfig,
  profileId: string,
  bundle: Bundle,
  yours: readonly DataSource[],
  pictures: Readonly<Record<string, string>> = {},
): { config: WireConfig; sources: DataSource[]; pageId: string } {
  const pageIds: string[] = [];
  const widgetIds: string[] = [];
  for (const profile of config.profiles ?? []) {
    for (const page of profile.pages ?? []) {
      pageIds.push(page.id);
      for (const widget of page.widgets ?? []) widgetIds.push(widget.id);
    }
  }
  const { sources, renames } = importedSources(bundle, yours);

  let text = JSON.stringify(bundle.page);
  for (const [sha, image] of Object.entries(pictures)) text = text.replaceAll(`img:${sha}`, image);
  for (const [from, to] of renames) {
    text = text.replaceAll(`src.${from}.`, `src.${to}.`).replaceAll(`"source":"${from}"`, `"source":"${to}"`);
  }
  const page = JSON.parse(text) as WirePage;
  const pageId = nextPageId(pageIds);
  const widgets: WireWidget[] = (page.widgets ?? []).map((widget) => {
    const id = nextWidgetId(widgetIds);
    widgetIds.push(id);
    return { ...widget, id };
  });
  const fresh: WirePage = { ...page, id: pageId, widgets };

  let placed = false;
  const profiles = (config.profiles ?? []).map((profile) => {
    if (profile.id !== profileId) return profile;
    placed = true;
    return { ...profile, pages: [...(profile.pages ?? []), fresh] };
  });
  if (!placed) throw new Error("There is no such profile to add the page to.");
  const schema = Math.max(config.schema ?? 0, bundle.schema ?? 0);
  return { config: { ...config, schema, profiles }, sources, pageId };
}
