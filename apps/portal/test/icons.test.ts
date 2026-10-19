import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { ICONS, byCategory } from "../src/common/icons/icons.lib.ts";

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

test("pack icons group by category in the pack's order, the uncategorised last", () => {
  const plain: { name: string; category?: string }[] = [{ name: "a" }, { name: "b" }];
  assert.deepEqual(byCategory(plain), [[null, plain]]);
  assert.deepEqual(byCategory([]), []);
  const icons = [{ name: "fox", category: "Nature" }, { name: "x" }, { name: "dice", category: "Games" }, { name: "leaf", category: "Nature" }];
  assert.deepEqual(
    byCategory(icons).map(([category, group]) => [category, group.map((icon) => icon.name)]),
    [["Nature", ["fox", "leaf"]], ["Games", ["dice"]], ["Other", ["x"]]],
  );
});
