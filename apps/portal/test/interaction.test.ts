import assert from "node:assert/strict";
import { test } from "node:test";

import type { GridSpec } from "@flexdeck/grid";

import { addWidgetAt } from "../src/modules/config/lib/editing.lib.ts";
import {
  axisIndexAt,
  cellAt,
  emptyCells,
  resizedSpan,
} from "../src/modules/config/lib/interaction.lib.ts";
import type { WireConfig } from "../src/services/api/api.ts";

/**
 * Pointer-to-cell arithmetic and adding at a named cell.
 *
 * Every failure worth catching here is an off-by-one at an edge or inside a gap,
 * so the tests are mostly about the first cell, the last cell, and the gap
 * between two of them.
 */

const spec = (overrides: Partial<GridSpec> = {}): GridSpec => ({
  cols: 4,
  rows: 3,
  gapX: 0,
  gapY: 0,
  usableW: 400,
  usableH: 300,
  ...overrides,
});

function config(): WireConfig {
  return {
    schema: 2,
    rev: 1,
    profiles: [
      {
        id: "tablet",
        grid: { cols: 4, rows: 3, usableW: 400, usableH: 300 },
        pages: [{ id: "page-1", name: "Home", widgets: [{ id: "widget-1", col: 0, row: 0 }] }],
      },
    ],
  };
}

function placed(document: WireConfig) {
  return document.profiles?.[0]?.pages?.[0]?.widgets?.[1];
}

// -- axisIndexAt -------------------------------------------------------------

test("a point before the first cell belongs to the first cell", () => {
  assert.equal(axisIndexAt([0, 100, 200], -50), 0);
});

test("a point on a cell's leading edge belongs to that cell", () => {
  assert.equal(axisIndexAt([0, 100, 200], 100), 1);
  assert.equal(axisIndexAt([0, 100, 200], 200), 2);
});

test("a point past the last cell stays on the last cell, not off the grid", () => {
  assert.equal(axisIndexAt([0, 100, 200], 9999), 2);
});

test("a point inside a GAP belongs to the cell on its left", () => {
  // Two 35px cells at offsets 10 and 55, with a 10px gap between 45 and 55.
  assert.equal(axisIndexAt([10, 55], 40), 0);
  assert.equal(axisIndexAt([10, 55], 50), 0);
  assert.equal(axisIndexAt([10, 55], 55), 1);
});

test("an empty axis answers 0 rather than throwing", () => {
  assert.equal(axisIndexAt([], 123), 0);
});

// -- cellAt ------------------------------------------------------------------

test("cellAt maps a point to its column and row", () => {
  assert.deepEqual(cellAt(spec(), 250, 150), { col: 2, row: 1 });
  assert.deepEqual(cellAt(spec(), 0, 0), { col: 0, row: 0 });
  assert.deepEqual(cellAt(spec(), 399, 299), { col: 3, row: 2 });
});

test("cellAt clamps a point outside the grid to the nearest cell", () => {
  assert.deepEqual(cellAt(spec(), -20, -20), { col: 0, row: 0 });
  assert.deepEqual(cellAt(spec(), 9999, 9999), { col: 3, row: 2 });
});

// -- resizedSpan -------------------------------------------------------------

test("a resize grows to the cell under the pointer, inclusively", () => {
  assert.deepEqual(resizedSpan({ col: 1, row: 1 }, spec(), { col: 3, row: 2 }), { w: 3, h: 2 });
});

test("dragging the grip above or to the LEFT of the origin shrinks to one cell", () => {
  assert.deepEqual(resizedSpan({ col: 2, row: 2 }, spec(), { col: 0, row: 0 }), { w: 1, h: 1 });
});

test("a resize is clamped to the grid's edge, never past it", () => {
  assert.deepEqual(resizedSpan({ col: 1, row: 1 }, spec(), { col: 99, row: 99 }), { w: 3, h: 2 });
});

test("a resize from the last cell is exactly one cell", () => {
  assert.deepEqual(resizedSpan({ col: 3, row: 2 }, spec(), { col: 3, row: 2 }), { w: 1, h: 1 });
});

// -- emptyCells --------------------------------------------------------------

test("a spanning widget occupies every cell it covers", () => {
  assert.deepEqual(emptyCells(spec({ cols: 2, rows: 2 }), []), [
    { col: 0, row: 0 },
    { col: 1, row: 0 },
    { col: 0, row: 1 },
    { col: 1, row: 1 },
  ]);

  // A 2x2 widget leaves nothing, and a 1x2 widget removes two cells, not one.
  assert.deepEqual(emptyCells(spec({ cols: 2, rows: 2 }), [{ id: "a", col: 0, row: 0, w: 2, h: 2 }]), []);
  assert.deepEqual(
    emptyCells(spec({ cols: 2, rows: 3 }), [{ id: "a", col: 0, row: 0, w: 1, h: 2 }]),
    [
      { col: 1, row: 0 },
      { col: 1, row: 1 },
      { col: 0, row: 2 },
      { col: 1, row: 2 },
    ],
  );
});

test("the new widget on a mostly full grid takes the last remaining cell", () => {
  assert.deepEqual(emptyCells(spec({ cols: 2, rows: 1 }), [{ id: "a", col: 0, row: 0, w: 1, h: 1 }]), [
    { col: 1, row: 0 },
  ]);
});

// -- addWidgetAt -------------------------------------------------------------

test("addWidgetAt places the widget at exactly the named cell", () => {
  const next = addWidgetAt(config(), "page-1", 2, 1, {
    type: "WIDGET_TYPE_BUTTON",
    props: { label: "New button" },
  });
  const widget = placed(next);

  assert.ok(widget);
  assert.equal(widget.col, 2);
  assert.equal(widget.row, 1);
  assert.equal(widget.w, 1);
  assert.equal(widget.h, 1);
  assert.equal(widget.props?.label, "New button");
  assert.notEqual(widget.id, "widget-1", "the new widget needs its own id");
});

test("addWidgetAt refuses an occupied cell by returning the same object", () => {
  const before = config();
  assert.equal(
    addWidgetAt(before, "page-1", 0, 0, { type: "WIDGET_TYPE_BUTTON" }),
    before,
  );
});

test("addWidgetAt refuses a cell off the grid", () => {
  const before = config();
  assert.equal(
    addWidgetAt(before, "page-1", 4, 0, { type: "WIDGET_TYPE_BUTTON" }),
    before,
  );
});

test("addWidgetAt refuses an unknown page", () => {
  const before = config();
  assert.equal(
    addWidgetAt(before, "page-nope", 1, 0, { type: "WIDGET_TYPE_BUTTON" }),
    before,
  );
});
