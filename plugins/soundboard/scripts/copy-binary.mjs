// Put the release binary where plugin.json says it is: bin/flexdeck-soundboard(.exe).
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const name = process.platform === "win32" ? "flexdeck-soundboard.exe" : "flexdeck-soundboard";
const from = join(here, "..", "..", "..", "target", "release", name);
const to = join(here, "..", "bin", name);
mkdirSync(dirname(to), { recursive: true });
copyFileSync(from, to);
console.log(`copied ${name} into bin/`);
