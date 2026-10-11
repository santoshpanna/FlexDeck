import assert from "node:assert/strict";
import { test } from "node:test";

import { cn } from "../src/lib/cn.ts";

test("a later utility replaces a conflicting earlier one", () => {
  assert.equal(cn("h-10 px-4", "h-12"), "px-4 h-12");
});

test("falsy entries are dropped", () => {
  assert.equal(cn("a", false, null, undefined, "", "b"), "a b");
});

test("conditional objects are honoured", () => {
  assert.equal(cn("base", { on: true, off: false }), "base on");
});

test("theme colours merge like built-in ones", () => {
  assert.equal(cn("text-muted", "text-ink"), "text-ink");
});
