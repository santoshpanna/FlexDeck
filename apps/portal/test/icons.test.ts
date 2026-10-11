import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { ICONS } from "../src/common/icons/icons.lib.ts";

test("the tablet's icon asset is the editor's icon set", () => {
  const asset = JSON.parse(
    readFileSync(new URL("../../../android/app/src/main/assets/icons.json", import.meta.url), "utf8"),
  ) as Record<string, string[]>;
  assert.deepEqual(
    asset,
    Object.fromEntries(ICONS.map((icon) => [icon.name, [...icon.paths]])),
    "run `node scripts/export-icons.mjs` after changing the icons",
  );
});
