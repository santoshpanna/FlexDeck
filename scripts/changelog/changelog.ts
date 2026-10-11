/**
 * Regenerate the changelog from git history.
 *
 * Writes:
 *   apps/portal/src/modules/changelog/releases.json   what the /changelog page renders
 *   CHANGELOG.md                                      the same, for GitHub
 *
 * The output depends only on commits — no timestamps — so running it twice
 * changes nothing, and CI commits only when the history really moved.
 *
 * Run: pnpm changelog            write both files
 *      pnpm changelog --check    exit 1 if either file is stale
 *      pnpm changelog --tag      also create a local tag for each untagged release commit
 *      pnpm changelog --notes v0.2.0 [--title]
 *                                print one release's notes (or its title) and write nothing
 */
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { type Commit, buildChangelog, releaseMarkdown, toMarkdown } from "./changelog.lib.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const git = (...args: string[]) => execFileSync("git", args, { cwd: root, encoding: "utf8" });

const RS = "\x1e";
const US = "\x1f";

/** Every commit on HEAD, oldest first. */
function commits(): Commit[] {
  return git("log", "--reverse", `--format=%H${US}%cs${US}%B${RS}`, "HEAD")
    .split(RS)
    .map((row) => row.trim())
    .filter(Boolean)
    .map((row) => {
      const [hash, date, message] = row.split(US);
      return { hash, date, message: message.trim() };
    });
}

const log = buildChangelog(commits());
const { unknownScopes, ...published } = log;

const notesFor = process.argv.indexOf("--notes");
if (notesFor >= 0) {
  const version = process.argv[notesFor + 1];
  const release = log.releases.find((candidate) => candidate.version === version);
  if (!release) {
    console.error(`changelog: no release ${version} in the history`);
    process.exit(1);
  }
  process.stdout.write(process.argv.includes("--title") ? `${release.title || release.version}\n` : releaseMarkdown(release));
  process.exit(0);
}
const outputs: [string, string][] = [
  [join(root, "apps/portal/src/modules/changelog/releases.json"), `${JSON.stringify(published, null, 2)}\n`],
  [join(root, "CHANGELOG.md"), toMarkdown(published)],
];

for (const scope of unknownScopes) console.warn(`changelog: unknown scope "${scope}" filed under GENERAL — add it to AREAS in changelog.lib.ts`);

if (process.argv.includes("--check")) {
  const stale = outputs.filter(([path, content]) => !existsSync(path) || readFileSync(path, "utf8") !== content);
  for (const [path] of stale) console.error(`changelog: ${path} is stale — run pnpm changelog`);
  process.exit(stale.length > 0 ? 1 : 0);
}

for (const [path, content] of outputs) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
}

if (process.argv.includes("--tag")) {
  const existing = new Set(git("tag", "--list", "v*").split(/\r?\n/).filter(Boolean));
  for (const release of log.releases) {
    if (existing.has(release.version)) continue;
    git("tag", "-a", release.version, release.hash, "-m", release.title || release.version);
    console.log(`changelog: tagged ${release.version} at ${release.hash.slice(0, 7)}`);
  }
}

console.log(`changelog: ${log.releases.length} release(s), ${log.next.reduce((sum, group) => sum + group.items.length, 0)} unreleased change(s)`);
