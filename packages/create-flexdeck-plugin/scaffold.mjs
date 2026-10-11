// Copy a plugin template into a new folder, with its id and name replaced.
// Used by `npm create flexdeck-plugin` and by the monorepo's
// `pnpm create-plugin`, so both make the same plugin.
import { copyFileSync, existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export const ID = /^[a-z][a-z0-9_-]{0,63}$/;

/** What is never copied: the template's own build, packages and notes. */
const SKIP = new Set(["node_modules", "dist", "README.md"]);

/**
 * Copy `from` (a plugin template) into `to`, naming the plugin `id` and
 * `name`. `sdk` is the version range of @flexdeck/plugin-sdk to depend on;
 * absent, the template's own (the monorepo's `workspace:*`) is kept.
 */
export function scaffold({ from, to, id, name = id, sdk }) {
  if (!ID.test(id)) throw new Error("an id is lowercase letters, digits, - and _, starting with a letter");
  if (existsSync(to)) throw new Error(`${to} already exists`);
  copyTree(from, to, (name) => !SKIP.has(name) && !name.endsWith(".fdpkg"));
  rewrite(to, (text) => {
    let next = text
      .replaceAll('"template"', `"${id}"`)
      .replaceAll("template.", `${id}.`)
      .replaceAll("@flexdeck/plugin-template", `@flexdeck/plugin-${id}`)
      .replaceAll('"Template"', JSON.stringify(name))
      .replaceAll("template-0.1.0", `${id}-0.1.0`);
    if (sdk) next = next.replaceAll('"@flexdeck/plugin-sdk": "workspace:*"', `"@flexdeck/plugin-sdk": "${sdk}"`);
    return next;
  });
  const readme = `# ${name}\n\nA FlexDeck plugin.\n\n    npm install\n    npm test            run its tests, without the agent\n    npm run package     write ${id}-0.1.0.fdpkg, ready to install\n\nWhile writing it, link this folder from the FlexDeck window (Plugins → Link a\nfolder): every save reloads it in the running agent.\n`;
  writeFileSync(join(to, "README.md"), readme);
}

/**
 * Copy the tree at `from` into `to`, entries `keep` refuses left out — before
 * they are looked into, so a linked node_modules is never followed. Links are
 * not copied.
 */
export function copyTree(from, to, keep) {
  mkdirSync(to, { recursive: true });
  for (const name of readdirSync(from)) {
    if (!keep(name)) continue;
    const source = join(from, name);
    const stat = lstatSync(source);
    if (stat.isSymbolicLink()) continue;
    if (stat.isDirectory()) copyTree(source, join(to, name), keep);
    else copyFileSync(source, join(to, name));
  }
}

function rewrite(directory, change) {
  for (const entry of readdirSync(directory)) {
    const path = join(directory, entry);
    if (statSync(path).isDirectory()) {
      rewrite(path, change);
      continue;
    }
    const text = readFileSync(path, "utf8");
    const next = change(text);
    if (next !== text) writeFileSync(path, next);
  }
}
