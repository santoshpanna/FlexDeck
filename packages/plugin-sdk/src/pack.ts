/**
 * Turn a plugin project into a `.fdpkg` the agent can install.
 *
 * 1. Read `plugin.json` and check it the way the agent will.
 * 2. Bundle `src/index.ts` into the declared entry (one file, SDK included),
 *    so the package needs no `node_modules`.
 * 3. Load the bundle and compare what it *says* it is with `plugin.json` —
 *    the agent refuses a mismatch at install, so catching it here saves a trip.
 * 4. Zip `plugin.json`, the entry, and `assets/` and `README.md` if present.
 */

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

import { ABI_VERSION, isValidId, type PackageManifest, type Permission, SUPPORTED_ABIS } from "./protocol.ts";
import type { Plugin } from "./index.ts";
import { zip, type ZipEntry } from "./zip.ts";

const PERMISSIONS: Permission[] = ["network", "notifications", "processes", "filesystem"];

export interface PackResult {
  file: string;
  sha256: string;
  size: number;
  package: PackageManifest;
}

export function readPackage(directory: string): PackageManifest {
  const path = join(directory, "plugin.json");
  if (!existsSync(path)) throw new Error(`there is no plugin.json in ${directory}`);
  // Decoded strictly: the agent refuses a plugin.json that is not UTF-8, so a
  // lenient read here would pack something that can never install.
  let text: string;
  try {
    text = new TextDecoder("utf-8", { fatal: true }).decode(readFileSync(path));
  } catch {
    throw new Error("plugin.json is not UTF-8; save it as UTF-8");
  }
  const manifest = JSON.parse(text) as PackageManifest;
  const problems: string[] = [];
  if (!SUPPORTED_ABIS.includes(manifest.abi)) problems.push(`abi must be one of ${SUPPORTED_ABIS.join(", ")} (${ABI_VERSION} is newest), not ${manifest.abi}`);
  if (!isValidId(manifest.id ?? "")) problems.push(`'${manifest.id}' is not a usable id`);
  if (!manifest.name) problems.push("name is required");
  if (!manifest.version) problems.push("version is required");
  if (manifest.runtime !== "node" && manifest.runtime !== "native" && manifest.runtime !== "recipe") problems.push("runtime must be 'node', 'native' or 'recipe'");
  if (manifest.runtime === "recipe" && !manifest.recipe) problems.push("a recipe needs a 'recipe' file");
  if (manifest.runtime === "recipe" && manifest.abi < 4) problems.push("a recipe needs abi 4");
  if (manifest.runtime === "node" && !manifest.entry) problems.push("a node plugin needs an 'entry'");
  for (const permission of manifest.permissions ?? []) {
    if (!PERMISSIONS.includes(permission)) problems.push(`'${permission}' is not a permission (${PERMISSIONS.join(", ")})`);
  }
  if (problems.length > 0) throw new Error(`plugin.json: ${problems.join("; ")}`);
  return manifest;
}

/** Bundle `src/index.ts` into the entry, if there is a source to bundle. */
export async function bundle(directory: string, manifest: PackageManifest): Promise<void> {
  const source = join(directory, "src", "index.ts");
  if (manifest.runtime !== "node" || !manifest.entry || !existsSync(source)) return;
  const esbuild = await import("esbuild");
  await esbuild.build({
    entryPoints: [source],
    outfile: join(directory, manifest.entry),
    bundle: true,
    platform: "node",
    format: "esm",
    target: "node22",
    legalComments: "none",
    logLevel: "warning",
    // A stable bundle is what makes a stable checksum.
    banner: { js: `// ${manifest.id} ${manifest.version} — built with @flexdeck/plugin-sdk` },
  });
}

/** Load the built entry and compare it with `plugin.json`. */
export async function verify(directory: string, manifest: PackageManifest): Promise<void> {
  if (manifest.runtime !== "node" || !manifest.entry) return;
  const loaded = (await import(pathToFileURL(resolve(directory, manifest.entry)).href)) as { default?: Plugin };
  const plugin = loaded.default;
  if (!plugin || typeof plugin.manifest !== "function") {
    throw new Error(`${manifest.entry} must 'export default definePlugin({...})'`);
  }
  const declared = plugin.manifest();
  const mismatches: string[] = [];
  if (declared.id !== manifest.id) mismatches.push(`id: plugin.json '${manifest.id}', code '${declared.id}'`);
  if (declared.version !== manifest.version) {
    mismatches.push(`version: plugin.json '${manifest.version}', code '${declared.version}'`);
  }
  const allowed = new Set(manifest.permissions ?? []);
  for (const permission of declared.permissions) {
    if (!allowed.has(permission)) mismatches.push(`the code asks for '${permission}', which plugin.json does not declare`);
  }
  if (mismatches.length > 0) throw new Error(`plugin.json and the code disagree: ${mismatches.join("; ")}`);
}

function walk(root: string, directory: string, into: ZipEntry[]): void {
  for (const name of readdirSync(directory)) {
    const path = join(directory, name);
    if (statSync(path).isDirectory()) {
      walk(root, path, into);
    } else {
      into.push({ name: relative(root, path).split(sep).join("/"), data: readFileSync(path) });
    }
  }
}

export async function pack(directory: string, options: { out?: string; build?: boolean } = {}): Promise<PackResult> {
  const root = resolve(directory);
  const manifest = readPackage(root);
  if (options.build !== false) await bundle(root, manifest);
  await verify(root, manifest);

  const entries: ZipEntry[] = [{ name: "plugin.json", data: readFileSync(join(root, "plugin.json")) }];
  const files =
    manifest.runtime === "node"
      ? [manifest.entry as string]
      : manifest.runtime === "recipe"
        ? [manifest.recipe as string]
        : Object.values(manifest.entries ?? {});
  for (const file of files) {
    const path = join(root, file);
    if (!existsSync(path)) throw new Error(`the entry ${file} does not exist — did the build run?`);
    entries.push({ name: file.split(sep).join("/").replace(/^\.\//, ""), data: readFileSync(path) });
  }
  if (existsSync(join(root, "assets"))) walk(root, join(root, "assets"), entries);
  if (existsSync(join(root, "README.md"))) entries.push({ name: "README.md", data: readFileSync(join(root, "README.md")) });

  const bytes = zip(entries);
  const file = options.out ?? join(root, `${manifest.id}-${manifest.version}.fdpkg`);
  writeFileSync(file, bytes);
  return {
    file,
    sha256: createHash("sha256").update(bytes).digest("hex"),
    size: bytes.length,
    package: manifest,
  };
}

