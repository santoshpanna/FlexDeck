#!/usr/bin/env node
// npm create flexdeck-plugin <id> ["<Name>"]
//
// A new FlexDeck plugin in ./<id>: the same template the FlexDeck repository
// tests, ready to `npm install`, test and pack.
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { ID, scaffold } from "./scaffold.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const [id, name = id] = process.argv.slice(2);
if (!id || !ID.test(id)) {
  console.error('usage: npm create flexdeck-plugin <id> "<Name>"   (id: lowercase letters, digits, - and _)');
  process.exit(1);
}
const own = JSON.parse(readFileSync(join(here, "package.json"), "utf8"));
try {
  scaffold({ from: join(here, "template"), to: resolve(id), id, name, sdk: `^${own.sdkVersion}` });
} catch (error) {
  console.error(String(error instanceof Error ? error.message : error));
  process.exit(1);
}
console.log(`created ./${id}`);
console.log("next:");
console.log(`  cd ${id}`);
console.log("  npm install");
console.log("  npm test");
console.log("  npm run package");
