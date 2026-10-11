import assert from "node:assert/strict";
import { test } from "node:test";

import { DOT_WINDOW, pageDots, pageLabel } from "../src/modules/config/lib/dots.lib.ts";

/**
 * The page-dot rail.
 *
 * Every interesting failure here is an off-by-one at an end, so the tests are
 * mostly about the first page, the last page, and the boundary where the window
 * starts to slide.
 */

test("a deck that fits shows every dot and no gaps", () => {
  const window = pageDots(4, 2);
  assert.deepEqual(window.indices, [0, 1, 2, 3]);
  assert.equal(window.gapBefore, false);
  assert.equal(window.gapAfter, false);
});

test("exactly the window size still shows every dot", () => {
  const window = pageDots(DOT_WINDOW, 3);
  assert.deepEqual(window.indices, [0, 1, 2, 3, 4, 5, 6]);
  assert.equal(window.indices.length, DOT_WINDOW);
  assert.equal(window.gapBefore, false);
  assert.equal(window.gapAfter, false);
});

test("a single page is just one dot, and no pages is an empty window", () => {
  assert.deepEqual(pageDots(1, 0).indices, [0]);
  assert.deepEqual(pageDots(0, 0).indices, []);
});

test("on the first page the window is anchored to the start", () => {
  const window = pageDots(12, 0);
  assert.deepEqual(window.indices, [0, 1, 2, 3, 4, 5, 6]);
  assert.equal(window.gapBefore, false);
  assert.equal(window.gapAfter, true);
});

test("on the last page the window is anchored to the end", () => {
  const window = pageDots(12, 11);
  assert.deepEqual(window.indices, [5, 6, 7, 8, 9, 10, 11]);
  assert.equal(window.gapBefore, true);
  assert.equal(window.gapAfter, false);
});

test("in the middle the window straddles the current page", () => {
  const window = pageDots(12, 5);
  assert.deepEqual(window.indices, [2, 3, 4, 5, 6, 7, 8]);
  assert.equal(window.gapBefore, true);
  assert.equal(window.gapAfter, true);
});

test("the window is always contiguous and never longer than asked for", () => {
  for (let current = 0; current < 30; current += 1) {
    const window = pageDots(30, current);
    assert.ok(window.indices.length <= DOT_WINDOW, `too many dots at ${current}`);
    for (let i = 1; i < window.indices.length; i += 1) {
      assert.equal(window.indices[i], window.indices[i - 1] + 1, `gap at ${current}`);
    }
    assert.ok(window.indices.includes(current), `current page missing at ${current}`);
  }
});

test("a window smaller than three falls back to showing everything", () => {
  const window = pageDots(12, 5, 2);
  assert.deepEqual(window.indices, Array.from({ length: 12 }, (_, index) => index));
});

test("an out-of-range current page is clamped rather than trusted", () => {
  assert.deepEqual(pageDots(5, 99).indices, [0, 1, 2, 3, 4]);
  assert.deepEqual(pageDots(5, -3).indices, [0, 1, 2, 3, 4]);
  assert.deepEqual(pageDots(12, 99).indices, [5, 6, 7, 8, 9, 10, 11]);
});

test("the overflow label is one-based and clamped", () => {
  assert.equal(pageLabel(0, 12), "1 / 12");
  assert.equal(pageLabel(11, 12), "12 / 12");
  assert.equal(pageLabel(99, 12), "12 / 12");
});
