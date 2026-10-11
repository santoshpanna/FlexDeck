/**
 * Turn a release tag into what each build needs.
 *
 *   node scripts/release/version.ts v0.2.1            prints name=0.2.1 and code=201 (for $GITHUB_OUTPUT)
 *   node scripts/release/version.ts v0.2.1 --desktop  also writes the version into tauri.conf.json
 *
 * The Android versionCode must only ever grow, so it is derived from the
 * version itself: major·10000 + minor·100 + patch. That caps minor and patch at
 * 99, which a release cadence will not reach.
 */
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export function parseVersion(tag: string): { name: string; code: number } {
  const match = /^v?(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?$/.exec(tag);
  if (!match) throw new Error(`not a release version: ${tag}`);
  const [major, minor, patch] = match.slice(1, 4).map(Number);
  if (minor > 99 || patch > 99) throw new Error(`${tag}: minor and patch must stay under 100 for the Android versionCode`);
  return { name: tag.replace(/^v/, ""), code: major * 10000 + minor * 100 + patch };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const tag = process.argv[2];
  if (!tag) {
    console.error("usage: node scripts/release/version.ts <vX.Y.Z> [--desktop]");
    process.exit(1);
  }
  const { name, code } = parseVersion(tag);
  if (process.argv.includes("--desktop")) {
    const path = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "apps", "desktop", "src-tauri", "tauri.conf.json");
    const config = JSON.parse(readFileSync(path, "utf8"));
    // NSIS rejects a pre-release suffix in the file version; the installer carries the plain number.
    config.version = name.replace(/-.*$/, "");
    writeFileSync(path, `${JSON.stringify(config, null, 2)}\n`);
  }
  console.log(`name=${name}`);
  console.log(`code=${code}`);
}
