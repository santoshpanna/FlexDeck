/**
 * Build a plugin repository: `repository.min.json` (what agents read) and
 * `repository.json` (the same, readable), from a folder of `.fdpkg` files.
 *
 * A repository lists each plugin once, at its highest version. The index is a
 * static file, so a repository can live anywhere that serves files — GitHub
 * Pages, a raw GitHub URL, any web server. Addresses in it are written
 * relative to the index, so the folder can move without a rebuild.
 *
 * Running it again over a folder that already has an index keeps history:
 * `releaseDate` stays the day the plugin first appeared, `latestUpdateDate`
 * moves only when the version does, and a version that reappears with
 * different bytes is refused — a published version never changes, or its
 * checksum stops meaning anything.
 */

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";

import { isValidId, type PackageManifest } from "./protocol.ts";
import { unzipEntry } from "./zip.ts";

export const REPO_SCHEMA = 1;

export interface RepoPackage {
  url: string;
  sha256: string;
  size: number;
}

export interface RepoPlugin {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  homepage?: string;
  abi: number;
  permissions: string[];
  releaseDate: string;
  latestUpdateDate: string;
  icon?: string;
  icons?: Record<string, string>;
  package: RepoPackage;
}

export interface RepoIndex {
  schema: number;
  repo: { name: string; description: string; website?: string; icon?: string };
  plugins: RepoPlugin[];
}

export interface RepoOptions {
  /** Where the index is written; addresses are relative to it. Default: the packages folder. */
  out?: string;
  name?: string;
  description?: string;
  website?: string;
  /** A folder of `<id>.png` and `<id>-<size>.png` icons. */
  icons?: string;
  /** The time stamped on new versions. Default: now. */
  now?: Date;
}

/** Compare versions so `1.10.0` sorts above `1.9.0`. Mirrors the agent's `version_order`. */
export function versionOrder(left: string, right: string): number {
  const parts = (text: string) => text.split(/[.+-]/).map((part) => (/^\d+$/.test(part) ? Number(part) : part));
  const a = parts(left);
  const b = parts(right);
  for (let index = 0; index < Math.min(a.length, b.length); index++) {
    const x = a[index]!;
    const y = b[index]!;
    if (x === y) continue;
    if (typeof x === "number" && typeof y === "number") return x - y;
    if (typeof x === "number") return 1;
    if (typeof y === "number") return -1;
    return x < y ? -1 : 1;
  }
  // `1.0.0-beta` is before `1.0.0`; `1.0.0.1` is after it.
  const extra = a.length > b.length ? a[b.length] : b.length > a.length ? b[a.length] : undefined;
  if (extra === undefined) return 0;
  const longerIsLater = typeof extra === "number";
  return (a.length > b.length) === longerIsLater ? 1 : -1;
}

/** A path relative to the index, with forward slashes whatever the OS. */
function address(from: string, to: string): string {
  return relative(from, to).split(sep).join("/");
}

function readManifest(file: string, bytes: Buffer): PackageManifest {
  const raw = unzipEntry(bytes, "plugin.json");
  if (!raw) throw new Error(`${file} has no plugin.json at its root`);
  const manifest = JSON.parse(raw.toString("utf8")) as PackageManifest;
  if (!isValidId(manifest.id ?? "")) throw new Error(`${file}: '${manifest.id}' is not a usable id`);
  if (!/^\d[0-9A-Za-z.+-]*$/.test(manifest.version ?? "") || manifest.version.includes("..")) {
    throw new Error(`${file}: '${manifest.version}' is not a usable version`);
  }
  return manifest;
}

function previousIndex(out: string): RepoIndex | null {
  for (const name of ["repository.json", "repository.min.json"]) {
    const path = join(out, name);
    if (existsSync(path)) return JSON.parse(readFileSync(path, "utf8")) as RepoIndex;
  }
  return null;
}

function iconsFor(id: string, iconDir: string | undefined, out: string): Pick<RepoPlugin, "icon" | "icons"> {
  if (!iconDir || !existsSync(iconDir)) return {};
  const result: Pick<RepoPlugin, "icon" | "icons"> = {};
  const sized: Record<string, string> = {};
  for (const name of readdirSync(iconDir).sort()) {
    const match = new RegExp(`^${id}(?:-(\\d+))?\\.(png|jpe?g|webp)$`).exec(name);
    if (!match) continue;
    const url = address(out, join(iconDir, name));
    if (match[1]) sized[match[1]] = url;
    else result.icon = url;
  }
  if (Object.keys(sized).length > 0) result.icons = sized;
  return result;
}

/** Build the index for every `.fdpkg` in `packages`, write both files, and return it. */
export function buildRepository(packages: string, options: RepoOptions = {}): RepoIndex {
  const packagesDir = resolve(packages);
  const out = resolve(options.out ?? packagesDir);
  const now = (options.now ?? new Date()).toISOString().replace(/\.\d{3}Z$/, "Z");
  const previous = previousIndex(out);
  const before = new Map((previous?.plugins ?? []).map((plugin) => [plugin.id, plugin]));

  // The highest version of each id wins.
  const latest = new Map<string, { file: string; bytes: Buffer; manifest: PackageManifest }>();
  for (const name of readdirSync(packagesDir).filter((name) => name.endsWith(".fdpkg")).sort()) {
    const file = join(packagesDir, name);
    const bytes = readFileSync(file);
    const manifest = readManifest(name, bytes);
    const current = latest.get(manifest.id);
    if (!current || versionOrder(manifest.version, current.manifest.version) > 0) {
      latest.set(manifest.id, { file, bytes, manifest });
    }
  }

  const plugins: RepoPlugin[] = [];
  for (const [id, { file, bytes, manifest }] of [...latest].sort(([a], [b]) => (a < b ? -1 : 1))) {
    const sha256 = createHash("sha256").update(bytes).digest("hex");
    const old = before.get(id);
    if (old && old.version === manifest.version && old.package.sha256 !== sha256) {
      throw new Error(
        `${id} ${manifest.version} is already published with different bytes; a published version never changes, so bump the version`,
      );
    }
    const entry: RepoPlugin = {
      id,
      name: manifest.name,
      version: manifest.version,
      description: manifest.description ?? "",
      author: manifest.author ?? "",
      ...(manifest.homepage ? { homepage: manifest.homepage } : {}),
      abi: manifest.abi,
      permissions: [...(manifest.permissions ?? [])],
      releaseDate: old?.releaseDate ?? now,
      latestUpdateDate: old && old.version === manifest.version ? old.latestUpdateDate : now,
      ...iconsFor(id, options.icons && resolve(options.icons), out),
      package: { url: address(out, file), sha256, size: bytes.length },
    };
    plugins.push(entry);
  }

  const index: RepoIndex = {
    schema: REPO_SCHEMA,
    repo: {
      name: options.name ?? previous?.repo.name ?? "My flexdeck plugins",
      description: options.description ?? previous?.repo.description ?? "",
      ...((options.website ?? previous?.repo.website) ? { website: options.website ?? previous?.repo.website } : {}),
      ...(previous?.repo.icon ? { icon: previous.repo.icon } : {}),
    },
    plugins,
  };
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, "repository.min.json"), JSON.stringify(index));
  writeFileSync(join(out, "repository.json"), `${JSON.stringify(index, null, 2)}\n`);
  return index;
}
