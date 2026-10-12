import assert from "node:assert/strict";
import { test } from "node:test";

import { childPath, containers, hintOf, openTo, search, sizeOf, type Json } from "../src/common/components/JsonTree/jsonTree.lib.ts";

const config: Json = {
  schema: 3,
  profiles: [
    {
      id: "p",
      pages: [{ id: "home", widgets: [{ id: "widget-11", props: { label: "Intermission" }, states: { variable: "obs.scene" } }] }],
    },
  ],
};

test("paths escape the separator, so two keys never share one", () => {
  assert.equal(childPath("", "a/b"), "a~1b");
  assert.equal(childPath("x", 0), "x/0");
});

test("only the shallow containers start open", () => {
  const open = openTo(config, 2);
  assert.ok(open.has(""));
  assert.ok(open.has("profiles"));
  assert.ok(!open.has("profiles/0"));
  assert.equal(containers(config).length, 9);
});

test("a folded container says its size, and an object names itself", () => {
  assert.equal(sizeOf([1, 2]), "2 items");
  assert.equal(sizeOf({ a: 1 }), "1 key");
  assert.equal(hintOf({ id: "widget-11", props: { label: "Intermission" } }), "widget-11 · Intermission");
  assert.equal(hintOf({ id: "home", name: "home" }), "home");
  assert.equal(hintOf([1]), null);
});

test("search finds keys and values, and opens the way down to them", () => {
  const { hits, open } = search(config, "obs.scene");
  assert.deepEqual([...hits], ["profiles/0/pages/0/widgets/0/states/variable"]);
  for (const path of ["", "profiles", "profiles/0", "profiles/0/pages/0/widgets/0", "profiles/0/pages/0/widgets/0/states"]) {
    assert.ok(open.has(path), path);
  }

  const byKey = search(config, "SCHEMA");
  assert.deepEqual([...byKey.hits], ["schema"]);
  assert.equal(search(config, "  ").hits.size, 0);
});
