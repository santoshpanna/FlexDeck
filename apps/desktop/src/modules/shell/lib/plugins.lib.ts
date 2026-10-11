import type { BuiltInPlugin, CatalogEntry, InstalledPlugin, RepoKind } from "./types.ts";

export interface PluginGroup {
  name: string;
  actions: number;
  detail: string;
}

/**
 * Describe the plugins this agent build ships.
 *
 * Built from the catalogue the agent actually serves, not from a list kept
 * here: adding a built-in on the Rust side shows up in the window with no
 * change to the window.
 */
export function pluginGroups(rows: readonly BuiltInPlugin[]): PluginGroup[] {
  return rows.map((row) => ({
    name: row.plugin,
    actions: row.actions,
    detail: count(row.actions),
  }));
}

export type Tone = "ok" | "warn" | "off";

/**
 * One word and a tone for an installed plugin, in the order a person needs
 * them: off on purpose, failed to start, running but unwell, or fine.
 */
export function pluginState(plugin: InstalledPlugin): { word: string; tone: Tone; reason: string | null } {
  if (!plugin.enabled) return { word: "Off", tone: "off", reason: null };
  if (plugin.error) return { word: "Failed", tone: "warn", reason: plugin.error };
  if (!plugin.running) return { word: "Stopped", tone: "warn", reason: plugin.status };
  if (plugin.status) return { word: "Trouble", tone: "warn", reason: plugin.status };
  return { word: "Running", tone: "ok", reason: null };
}

/** What a permission lets a plugin do, in words rather than an identifier. */
export function permissionLabel(permission: string): string {
  switch (permission) {
    case "network":
      return "Use the network — connect to other programs and the internet";
    case "notifications":
      return "Show notifications";
    case "processes":
      return "Start other programs";
    case "filesystem":
      return "Read and write files outside its own folder";
    default:
      return permission;
  }
}

/** Where an installed plugin came from, as a short phrase. */
export function sourceLabel(plugin: InstalledPlugin): string {
  switch (plugin.source.kind) {
    case "repository":
      return `from ${plugin.source.name ?? "a repository"}`;
    case "marketplace":
      return "from the marketplace";
    case "url":
      return "from a web address";
    case "dev":
      return `linked from ${plugin.source.path ?? "a folder"} — reloads on save`;
    default:
      return "from a file";
  }
}

export type MarketFilter = "all" | RepoKind | "updates";

/** What a marketplace entry's button says, and whether it can be pressed. */
export function entryAction(entry: CatalogEntry): { label: string; enabled: boolean; primary: boolean } {
  if (entry.update) return { label: "Update", enabled: true, primary: true };
  if (entry.installedVersion === null) return { label: "Install", enabled: true, primary: true };
  if (entry.installedFromHere) return { label: "Installed", enabled: false, primary: false };
  // The same id, installed from elsewhere: installing this one replaces it.
  return { label: "Replace", enabled: true, primary: false };
}

/** The entries a search and a filter chip leave, keeping the agent's order. */
export function filterCatalog(entries: readonly CatalogEntry[], query: string, filter: MarketFilter): CatalogEntry[] {
  const words = query.trim().toLowerCase();
  return entries.filter((entry) => {
    if (filter === "updates" ? !entry.update : filter !== "all" && entry.repo.kind !== filter) return false;
    if (words === "") return true;
    return [entry.id, entry.name, entry.description, entry.author].some((field) => field.toLowerCase().includes(words));
  });
}

/** How many entries each chip would show. */
export function filterCounts(entries: readonly CatalogEntry[]): Record<MarketFilter, number> {
  const counts: Record<MarketFilter, number> = { all: entries.length, official: 0, vetted: 0, custom: 0, updates: 0 };
  for (const entry of entries) {
    counts[entry.repo.kind] += 1;
    if (entry.update) counts.updates += 1;
  }
  return counts;
}

/** A repository kind as a person would say it. */
export function kindLabel(kind: RepoKind): string {
  switch (kind) {
    case "official":
      return "Official";
    case "vetted":
      return "Community";
    default:
      return "Custom";
  }
}

/** How long ago `when` was, in the largest unit that fits: "3 days ago". */
export function relativeDate(when: string | number | null | undefined, now: number = Date.now()): string {
  if (when === null || when === undefined || when === "") return "";
  // A number is seconds since the epoch, as the agent counts.
  const then = typeof when === "number" ? when * 1000 : Date.parse(when);
  if (Number.isNaN(then)) return "";
  const seconds = Math.max(0, Math.round((now - then) / 1000));
  if (seconds < 60) return "just now";
  const units: [number, string][] = [
    [60 * 60 * 24 * 365, "year"],
    [60 * 60 * 24 * 30, "month"],
    [60 * 60 * 24 * 7, "week"],
    [60 * 60 * 24, "day"],
    [60 * 60, "hour"],
    [60, "minute"],
  ];
  for (const [size, name] of units) {
    const count = Math.floor(seconds / size);
    if (count >= 1) return `${count} ${name}${count === 1 ? "" : "s"} ago`;
  }
  return "just now";
}

/** A package size in the unit a person reads. */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function count(actions: number): string {
  return actions === 1 ? "1 action" : `${actions} actions`;
}
