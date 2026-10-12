import assert from "node:assert/strict";
import { test } from "node:test";

import { paginate } from "../src/lib/paginate.ts";

const rows = Array.from({ length: 23 }, (_, index) => index + 1);

test("a middle page shows its slice and its range", () => {
  const paged = paginate(rows, 1, 8);
  assert.deepEqual(paged.items, [9, 10, 11, 12, 13, 14, 15, 16]);
  assert.equal(paged.pages, 3);
  assert.equal(paged.from, 9);
  assert.equal(paged.to, 16);
});

test("a page past the end steps back to the last one", () => {
  const paged = paginate(rows, 9, 8);
  assert.equal(paged.page, 2);
  assert.deepEqual(paged.items, [17, 18, 19, 20, 21, 22, 23]);
});

test("nothing to show is one empty page", () => {
  const paged = paginate([], 0, 8);
  assert.equal(paged.pages, 1);
  assert.equal(paged.from, 0);
  assert.equal(paged.to, 0);
});
