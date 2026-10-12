import assert from "node:assert/strict";
import { test } from "node:test";

import { formatCode } from "../src/modules/config/lib/code.lib.ts";

test("a dash goes in after the fourth character", () => {
  assert.equal(formatCode("554gz"), "554G-Z");
  assert.equal(formatCode("554gz4p5"), "554G-Z4P5");
});

test("a dash typed in the right place is kept, then not doubled", () => {
  assert.equal(formatCode("554g-"), "554G-");
  assert.equal(formatCode("554G-z"), "554G-Z");
});

test("a dash anywhere else, and spaces, are dropped", () => {
  assert.equal(formatCode("55-4"), "554");
  assert.equal(formatCode("5 54G Z4P5"), "554G-Z4P5");
});

test("deleting back to the dash and past it", () => {
  assert.equal(formatCode("554G-"), "554G-");
  assert.equal(formatCode("554G"), "554G");
});

test("letters mistaken for digits read as digits, and the length is capped", () => {
  assert.equal(formatCode("o1lI"), "0111");
  assert.equal(formatCode("554GZ4P5XX"), "554G-Z4P5");
});
