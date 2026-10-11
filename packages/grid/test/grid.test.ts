/**
 * Hand-verified unit tests for the grid algorithm.
 *
 * These check the algorithm is *correct* (easy to reason about by hand).
 * test/parity.test.ts separately checks it is *identical* to the Rust and
 * Kotlin implementations. Both matter; neither substitutes for the other.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  buildAxis,
  computeGrid,
  layoutPage,
  orientPage,
  orientSpan,
  orientSpec,
  rectOf,
} from "../src/index.ts";

test("buildAxis: 4x400 with no gap gives four 100px cells", () => {
  const axis = buildAxis(4, 400, 0);
  assert.deepEqual(axis.sizes, [100, 100, 100, 100]);
  assert.deepEqual(axis.offsets, [0, 100, 200, 300]);
});

test("buildAxis: gap is applied at both ends and between cells", () => {
  // 3 cells, gap 10 => 4 gaps = 40px of gap, 60px of cell => 20px each.
  const axis = buildAxis(3, 100, 10);
  assert.deepEqual(axis.sizes, [20, 20, 20]);
  assert.deepEqual(axis.offsets, [10, 40, 70]);
  // Leading gap + cells + inter-cell gaps + trailing gap == usable exactly.
  assert.equal(axis.offsets[2] + axis.sizes[2] + 10, 100);
});

test("buildAxis: remainder is distributed left to right", () => {
  // 101px across 3 cells => base 33, remainder 2 => [34, 34, 33].
  const axis = buildAxis(3, 101, 0);
  assert.deepEqual(axis.sizes, [34, 34, 33]);
  assert.deepEqual(axis.offsets, [0, 34, 68]);
  assert.equal(axis.offsets[2] + axis.sizes[2], 101);
});

test("buildAxis: larger remainder stays ordered, never exceeds usable", () => {
  const axis = buildAxis(7, 701, 6);
  const consumed = axis.offsets[6] + axis.sizes[6] + 6; // trailing gap
  assert.equal(consumed, 701);
  // Left-to-right distribution means sizes are non-increasing.
  for (let i = 1; i < axis.sizes.length; i++) {
    assert.ok(axis.sizes[i - 1]! >= axis.sizes[i]!, `sizes not non-increasing at ${i}`);
  }
});

test("buildAxis: degenerate input never produces negative sizes", () => {
  // 3 gaps of 10px cannot fit in 10px of usable space.
  const axis = buildAxis(2, 10, 10);
  assert.deepEqual(axis.sizes, [0, 0]);
  for (const size of axis.sizes) assert.ok(size >= 0);
});

test("buildAxis: zero cells is empty, not a crash", () => {
  assert.deepEqual(buildAxis(0, 400, 8), { offsets: [], sizes: [] });
});

test("rectOf: a full-span widget exactly fills the grid", () => {
  const spec = { cols: 6, rows: 3, gapX: 3, gapY: 3, usableW: 500, usableH: 300 };
  const rect = rectOf(computeGrid(spec), { id: "full", col: 0, row: 0, w: 6, h: 3 });
  assert.deepEqual(rect, { x: 3, y: 3, w: 500 - 6, h: 300 - 6 });
});

test("rectOf: a spanning widget includes the gaps it covers", () => {
  // 4 cells of 100px, no gap. Two-cell span == 200px, starting at 100px.
  const geometry = computeGrid({ cols: 4, rows: 4, gapX: 0, gapY: 0, usableW: 400, usableH: 400 });
  const rect = rectOf(geometry, { id: "b", col: 1, row: 1, w: 2, h: 2 });
  assert.deepEqual(rect, { x: 100, y: 100, w: 200, h: 200 });
});

test("rectOf: with a gap, a 2-cell span covers the inter-cell gap too", () => {
  // 3 cells of 20px with gap 10: offsets 10, 40, 70.
  // Cells 0..1 span from 10 to (40 + 20) = 60, so width 50 = 20 + 10 gap + 20.
  const geometry = computeGrid({ cols: 3, rows: 1, gapX: 10, gapY: 10, usableW: 100, usableH: 50 });
  assert.deepEqual(rectOf(geometry, { id: "pair", col: 0, row: 0, w: 2, h: 1 }), {
    x: 10,
    y: 10,
    w: 50,
    h: 30,
  });
});

test("layoutPage: lays out every widget, keyed by id", () => {
  const spec = { cols: 4, rows: 4, gapX: 0, gapY: 0, usableW: 400, usableH: 400 };
  const rects = layoutPage(spec, [
    { id: "a", col: 0, row: 0, w: 1, h: 1 },
    { id: "c", col: 3, row: 3, w: 1, h: 1 },
  ]);
  assert.deepEqual(Object.keys(rects).sort(), ["a", "c"]);
  assert.deepEqual(rects["a"], { x: 0, y: 0, w: 100, h: 100 });
  assert.deepEqual(rects["c"], { x: 300, y: 300, w: 100, h: 100 });
});

/* ---------------------------------------------------------------------------
 * Orientation.
 *
 * An asymmetric grid with two different gaps on purpose: a transform that forgot
 * to swap one axis would still pass on a square grid with equal gaps.
 * ------------------------------------------------------------------------- */

const landscape = { cols: 8, rows: 4, gapX: 3, gapY: 7, usableW: 1280, usableH: 800 };

test("orientSpec: landscape is the authored orientation, so it changes nothing", () => {
  assert.deepEqual(orientSpec(landscape, "landscape"), landscape);
});

test("orientSpec: portrait swaps the axes, the gaps AND the panel edges", () => {
  assert.deepEqual(orientSpec(landscape, "portrait"), {
    cols: 4,
    rows: 8,
    gapX: 7,
    gapY: 3,
    usableW: 800,
    usableH: 1280,
  });
});

test("orientSpan: portrait swaps the position and the extent together", () => {
  assert.deepEqual(orientSpan({ id: "a", col: 6, row: 1, w: 2, h: 3 }, "portrait"), {
    id: "a",
    col: 1,
    row: 6,
    w: 3,
    h: 2,
  });
});

test("orienting twice is the identity, so rotating loses nothing", () => {
  const widgets = [
    { id: "a", col: 0, row: 0, w: 2, h: 1 },
    { id: "b", col: 7, row: 3, w: 1, h: 1 },
  ];

  const once = orientPage(landscape, widgets, "portrait");
  const twice = orientPage(once.spec, once.widgets, "portrait");

  assert.deepEqual(twice.spec, landscape);
  assert.deepEqual(twice.widgets, widgets);
});

test("every widget still fits inside the grid after orienting", () => {
  const widgets = [
    { id: "a", col: 0, row: 0, w: 2, h: 1 },
    { id: "b", col: 6, row: 0, w: 2, h: 2 },
    { id: "c", col: 7, row: 3, w: 1, h: 1 },
  ];

  const portrait = orientPage(landscape, widgets, "portrait");
  for (const widget of portrait.widgets) {
    assert.ok(widget.col + widget.w <= portrait.spec.cols, `${widget.id}: escapes the columns`);
    assert.ok(widget.row + widget.h <= portrait.spec.rows, `${widget.id}: escapes the rows`);
  }
});

test("a widget spanning the full width becomes one spanning the full height", () => {
  const wide = [{ id: "bar", col: 0, row: 0, w: 8, h: 1 }];
  const before = layoutPage(landscape, wide)["bar"];

  const portrait = orientPage(landscape, wide, "portrait");
  const after = layoutPage(portrait.spec, portrait.widgets)["bar"];

  // The panel turned over...
  assert.equal(portrait.spec.usableW, 800);
  assert.equal(portrait.spec.usableH, 1280);
  // ...so what was as wide as the panel is now as tall as the panel is tall, and
  // the inter-cell gaps it swallowed swapped with it.
  assert.equal(before.w, 1280 - 2 * landscape.gapX);
  assert.equal(after.h, before.w);
});
