// The tablet draws the same icons the editor offers. They are defined once, in
// the portal (apps/portal/src/common/icons/icons.lib.ts); this writes them as
// the JSON asset the Android app reads. Run after changing the icon set; the
// portal's tests fail while the two disagree.
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { ICONS } from "../apps/portal/src/common/icons/icons.lib.ts";

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, "..", "android", "app", "src", "main", "assets", "icons.json");
const icons = Object.fromEntries(ICONS.map((icon) => [icon.name, icon.paths]));
writeFileSync(out, `${JSON.stringify(icons, null, 2)}\n`);
console.log(`wrote ${Object.keys(icons).length} icons to ${out}`);
