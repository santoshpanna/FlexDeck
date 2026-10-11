/**
 * The changelog, derived from git history.
 *
 * Commits here carry a plain subject and one typed line per change in the body:
 *
 *   Ship live config feed, fills, and active key lighting
 *   feat(portal): ship FillPicker with linear/radial gradients and angle dial
 *   feat(android): add connection panel and pairing polish
 *   ci: run workflow on dispatch only
 *
 * Every line — subject included — is read on its own. The type picks the group,
 * the scope picks the part of the product, and anything that is not a user-facing
 * type (`chore`, `ci`, `test`, `docs`, …) never reaches the page.
 *
 * A release is cut by a commit, not a tag. A `release: v0.1.0` line marks it; that
 * commit's subject is the release title and its untyped prose lines the blurb:
 *
 *   First public release
 *   release: v0.1.0
 *   The tablet in your drawer, turned into a control surface for your PC.
 *   feat(desktop): pair with six digits
 *
 * The release holds every change since the previous release commit, its own
 * lines included; whatever comes after the newest one is "next". CI tags the
 * release commit afterwards, so tags follow commits and never lead.
 *
 * Pure functions only: `changelog.ts` does the git calls and the writing.
 */

export type Area = "DESKTOP" | "ANDROID" | "EDITOR" | "PLUGINS" | "GENERAL";
export type Group = "Added" | "Improved" | "Fixed" | "Security";

export interface Entry {
  area: Area;
  text: string;
}

export interface Commit {
  hash: string;
  /** Committer date, YYYY-MM-DD. */
  date: string;
  message: string;
}

export interface Release {
  version: string;
  /** The release commit, so CI can tag it. */
  hash: string;
  date: string;
  title: string;
  blurb: string;
  groups: { name: Group; items: Entry[] }[];
}

export interface Changelog {
  next: Release["groups"];
  releases: Release[];
}

/** Type → group. Types not listed here are internal and dropped. */
const GROUPS: Record<string, Group> = {
  feat: "Added",
  perf: "Improved",
  fix: "Fixed",
  sec: "Security",
  security: "Security",
};

const GROUP_ORDER: Group[] = ["Added", "Improved", "Fixed", "Security"];

/** Plugin scopes get their name on the line, since the area alone says only "PLUGINS". */
const PLUGIN_NAMES: Record<string, string> = {
  discord: "Discord",
  obs: "OBS Studio",
  soundboard: "Soundboard",
  spotify: "Spotify",
  streamerbot: "Streamer.bot",
  weather: "Weather",
  template: "Plugin template",
};

/** Scope → area. Scopes in `INTERNAL` are dropped; anything unknown lands in GENERAL. */
const AREAS: Record<string, Area> = {
  desktop: "DESKTOP",
  server: "DESKTOP",
  agent: "DESKTOP",
  pairing: "DESKTOP",
  store: "DESKTOP",
  actions: "DESKTOP",
  automation: "DESKTOP",
  vars: "DESKTOP",
  expr: "DESKTOP",
  tunnel: "DESKTOP",
  android: "ANDROID",
  portal: "EDITOR",
  editor: "EDITOR",
  ui: "EDITOR",
  cloud: "EDITOR",
  grid: "EDITOR",
  face: "EDITOR",
  tokens: "EDITOR",
  plugin: "PLUGINS",
  plugins: "PLUGINS",
  sdk: "PLUGINS",
  abi: "PLUGINS",
  host: "PLUGINS",
  ...Object.fromEntries(Object.keys(PLUGIN_NAMES).map((id) => [id, "PLUGINS" as const])),
};

const INTERNAL = new Set(["proto", "protocol", "deps", "ci", "build", "repo", "test", "changelog"]);

const LINE = /^(\w+)(?:\(([^)]+)\))?!?:\s+(.+)$/;

export interface ParsedLine {
  group: Group;
  entry: Entry;
  /** Set when the scope was not recognised, so the caller can warn. */
  unknownScope?: string;
}

/** One commit line → one entry, or null when it is internal or not a typed line at all. */
export function parseLine(line: string): ParsedLine | null {
  const match = LINE.exec(line.trim());
  if (!match) return null;
  const [, type, rawScope, rawText] = match;
  const group = GROUPS[type.toLowerCase()];
  if (!group) return null;

  const scope = rawScope?.trim().toLowerCase();
  if (scope && INTERNAL.has(scope)) return null;

  const known = scope ? AREAS[scope] : undefined;
  const area: Area = known ?? "GENERAL";
  let text = rawText.trim().replace(/\.$/, "");
  text = scope && PLUGIN_NAMES[scope] ? `${PLUGIN_NAMES[scope]}: ${text}` : text.charAt(0).toUpperCase() + text.slice(1);

  return { group, entry: { area, text: `${text}.` }, ...(scope && !known ? { unknownScope: scope } : {}) };
}

/** A whole commit message (subject + body) → its entries, in the order written. */
export function parseMessage(message: string): ParsedLine[] {
  return message
    .split(/\r?\n/)
    .map(parseLine)
    .filter((parsed): parsed is ParsedLine => parsed !== null);
}

/** Entries → groups in a fixed order, duplicates dropped, empty groups left out. */
export function groupEntries(lines: ParsedLine[]): Release["groups"] {
  return GROUP_ORDER.map((name) => {
    const seen = new Set<string>();
    const items = lines
      .filter((line) => line.group === name)
      .map((line) => line.entry)
      .filter((entry) => {
        const key = `${entry.area}|${entry.text}`;
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
      });
    return { name, items };
  }).filter((group) => group.items.length > 0);
}

const RELEASE = /^release:\s*(v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)\s*$/i;
/** `Co-Authored-By: …`, `Signed-off-by: …` — git trailers, never prose. */
const TRAILER = /^[A-Z][\w-]*-[\w-]+:\s/;

/** The version a commit releases, or null when it is an ordinary commit. */
export function releaseVersion(message: string): string | null {
  for (const line of message.split(/\r?\n/)) {
    const match = RELEASE.exec(line.trim());
    if (match) return match[1];
  }
  return null;
}

/** A release commit's subject and its untyped prose lines. */
export function releaseNotes(message: string): { title: string; blurb: string } {
  const [subject = "", ...body] = message.split(/\r?\n/).map((line) => line.trim());
  const prose = body.filter((line) => line && !LINE.test(line) && !RELEASE.test(line) && !TRAILER.test(line));
  return { title: subject, blurb: prose.join(" ") };
}

/** Commits, oldest first → releases newest first, plus what is still unreleased. */
export function buildChangelog(commits: Commit[]): Changelog & { unknownScopes: string[] } {
  const releases: Release[] = [];
  const unknown = new Set<string>();
  const seen = new Set<string>();
  let pending: ParsedLine[] = [];

  for (const commit of commits) {
    const lines = parseMessage(commit.message);
    for (const line of lines) if (line.unknownScope) unknown.add(line.unknownScope);
    pending.push(...lines);

    const version = releaseVersion(commit.message);
    if (!version) continue;
    if (seen.has(version)) throw new Error(`changelog: ${version} is released twice (again in ${commit.hash.slice(0, 7)})`);
    seen.add(version);
    releases.push({ version, hash: commit.hash, date: commit.date, ...releaseNotes(commit.message), groups: groupEntries(pending) });
    pending = [];
  }

  return { next: groupEntries(pending), releases: releases.reverse(), unknownScopes: [...unknown] };
}

const GROUP_HEADINGS: Record<Group, string> = { Added: "Added", Improved: "Improved", Fixed: "Fixed", Security: "Security" };

const section = (groups: Release["groups"]) =>
  groups.map((group) => `### ${GROUP_HEADINGS[group.name]}\n\n${group.items.map((item) => `- **${item.area.toLowerCase()}** ${item.text}`).join("\n")}`).join("\n\n");

/** One release's notes, for its GitHub release page: the blurb, then what changed. */
export function releaseMarkdown(release: Release): string {
  return `${[release.blurb, section(release.groups)].filter(Boolean).join("\n\n")}\n`;
}

/** The same data as Markdown, for GitHub and the repo root. */
export function toMarkdown(log: Changelog): string {
  const parts = ["# Changelog", "Generated from commit history by `pnpm changelog`. Do not edit by hand."];
  if (log.next.length > 0) parts.push(`## Next\n\n${section(log.next)}`);
  for (const release of log.releases) {
    const heading = `## ${release.version} — ${release.date}`;
    const intro = [release.title && `**${release.title}**`, release.blurb].filter(Boolean).join("\n\n");
    parts.push([heading, intro, section(release.groups)].filter(Boolean).join("\n\n"));
  }
  return `${parts.join("\n\n")}\n`;
}
