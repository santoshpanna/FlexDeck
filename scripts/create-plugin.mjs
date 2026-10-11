#!/usr/bin/env node
// Copy plugins/template into plugins/<id>, with the id and name replaced.
//
//   pnpm create-plugin <id> "<Name>"
//
// The template is a real, tested plugin rather than a folder of placeholders,
// so what you copy is known to build, pass its tests, and pack. The copying
// is `npm create flexdeck-plugin`'s, so both make the same plugin.
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { ID, scaffold } from "../packages/create-flexdeck-plugin/scaffold.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const [id, name = id] = process.argv.slice(2);

if (!id || !ID.test(id)) {
  console.error('usage: pnpm create-plugin <id> "<Name>"   (id: lowercase letters, digits, - and _)');
  process.exit(1);
}
try {
  // No `sdk`: inside the repository the template's `workspace:*` stays.
  scaffold({ from: join(root, "plugins", "template"), to: join(root, "plugins", id), id, name });
} catch (error) {
  console.error(String(error instanceof Error ? error.message : error));
  process.exit(1);
}

console.log(`created plugins/${id}`);
console.log("next:");
console.log("  pnpm install");
console.log(`  pnpm --filter @flexdeck/plugin-${id} test`);
console.log(`  pnpm --filter @flexdeck/plugin-${id} run package`);
