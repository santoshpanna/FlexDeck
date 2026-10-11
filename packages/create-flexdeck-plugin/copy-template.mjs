// Bundle the repository's plugin template into this package before it is
// packed, so `npm create flexdeck-plugin` copies the tested one.
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { copyTree } from "./scaffold.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const from = join(here, "..", "..", "plugins", "template");
const to = join(here, "template");
rmSync(to, { recursive: true, force: true });
copyTree(from, to, (name) => !["node_modules", "dist"].includes(name) && !name.endsWith(".fdpkg"));
// npm drops .gitignore from packages; nothing else in the template is hidden.
const sdk = JSON.parse(readFileSync(join(here, "..", "plugin-sdk", "package.json"), "utf8")).version;
const own = JSON.parse(readFileSync(join(here, "package.json"), "utf8"));
if (own.sdkVersion !== sdk) {
  own.sdkVersion = sdk;
  writeFileSync(join(here, "package.json"), `${JSON.stringify(own, null, 2)}\n`);
}
console.log(`bundled the template (sdk ${sdk})`);
