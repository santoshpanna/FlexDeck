import assert from "node:assert/strict";
import { test } from "node:test";

import type { GridSpec } from "@flexdeck/grid";

import { describeGridProblem, gridProblem, setGridSpec } from "../src/modules/config/lib/grid.lib.ts";
import type { WireConfig } from "../src/services/api/api.ts";

/**
 * The grid's own size.
 *
 * The rule that matters is the refusal: shrinking the grid must not strand a
 * widget, on ANY page, and the wire shape must omit a zero gap because proto3
 * omits defaults.
 */

const spec = (overrides: Partial<GridSpec> = {}): GridSpec => ({
  cols: 4,
  rows: 3,
  gapX: 8,
  gapY: 8,
  usableW: 800,
  usableH: 600,
  ...overrides,
});

function config(): WireConfig {
  return {
    schema: 2,
    rev: 7,
    profiles: [
      {
        id: "tablet",
        name: "Tablet",
        grid: { cols: 4, rows: 3, gapX: 8, gapY: 8, usableW: 800, usableH: 600 },
        pages: [
          { id: "page-1", name: "Main", widgets: [{ id: "widget-1", col: 0, row: 0, w: 2, h: 1 }] },
          { id: "page-2", name: "Media", widgets: [{ id: "widget-2", col: 3, row: 2 }] },
        ],
      },
    ],
  };
}

// -- gridProblem -------------------------------------------------------------

test("a grid with no columns or no rows is too small", () => {
  assert.deepEqual(gridProblem(spec({ cols: 0 }), []), { kind: "too_small" });
  assert.deepEqual(gridProblem(spec({ rows: 0 }), []), { kind: "too_small" });
});

test("a grid that holds every widget on every page is fine", () => {
  assert.equal(gridProblem(spec(), config().profiles?.[0]?.pages ?? []), null);
});

test("shrinking counts the widgets that would fall off, across ALL pages", () => {
  const pages = config().profiles?.[0]?.pages ?? [];

  // widget-2 sits at col 3, so three columns strands it.
  assert.deepEqual(gridProblem(spec({ cols: 3 }), pages), { kind: "orphans_widgets", count: 1 });

  // One column strands both.
  assert.deepEqual(gridProblem(spec({ cols: 1 }), pages), { kind: "orphans_widgets", count: 2 });

  // Two rows strands widget-2 on row 2.
  assert.deepEqual(gridProblem(spec({ rows: 2 }), pages), { kind: "orphans_widgets", count: 1 });
});

test("the refusal explains itself in the singular and the plural", () => {
  assert.equal(
    describeGridProblem({ kind: "orphans_widgets", count: 1 }),
    "one widget would fall off the grid",
  );
  assert.equal(
    describeGridProblem({ kind: "orphans_widgets", count: 3 }),
    "3 widgets would fall off the grid",
  );
  assert.equal(
    describeGridProblem({ kind: "too_small" }),
    "a grid needs at least one column and one row",
  );
});

// -- setGridSpec -------------------------------------------------------------

test("setGridSpec changes the shape and preserves the panel size", () => {
  const next = setGridSpec(config(), { cols: 6, rows: 5 });
  const grid = next.profiles?.[0]?.grid;

  assert.equal(grid?.cols, 6);
  assert.equal(grid?.rows, 5);
  // The panel is a property of the device, not of this edit.
  assert.equal(grid?.usableW, 800);
  assert.equal(grid?.usableH, 600);
  assert.equal(grid?.gapX, 8);
});

test("setGridSpec writes a ZERO gap as absent, matching proto3's default omission", () => {
  const grid = setGridSpec(config(), { gapX: 0 }).profiles?.[0]?.grid;
  assert.ok(grid);
  assert.equal("gapX" in grid, false);
  assert.equal(grid.gapY, 8, "the untouched gap must survive");
});

test("setGridSpec refuses a shrink that would strand a widget, leaving the grid as it was", () => {
  const before = config();
  const after = setGridSpec(before, { cols: 3 });

  assert.equal(after, before, "a refusal is the same object");
  assert.equal(after.profiles?.[0]?.grid?.cols, 4);
});

test("setGridSpec allows shrinking to exactly fit", () => {
  const next = setGridSpec(config(), { rows: 3, cols: 4 });
  // Unchanged, so nothing to write.
  assert.equal(next.profiles?.[0]?.grid?.rows, 3);

  // And three columns is allowed once the far widget is gone.
  const withoutFarWidget: WireConfig = {
    schema: 2,
    rev: 1,
    profiles: [
      {
        id: "tablet",
        grid: { cols: 4, rows: 3, usableW: 800, usableH: 600 },
        pages: [{ id: "page-1", widgets: [{ id: "widget-1", col: 0, row: 0 }] }],
      },
    ],
  };
  assert.equal(setGridSpec(withoutFarWidget, { cols: 3 }).profiles?.[0]?.grid?.cols, 3);
});

test("setGridSpec refuses a grid with no columns", () => {
  const before = config();
  assert.equal(setGridSpec(before, { cols: 0 }), before);
});

test("setGridSpec returns the same object when nothing changed", () => {
  const before = config();
  assert.equal(setGridSpec(before, { cols: 4 }), before);
  assert.equal(setGridSpec(before, {}), before);
});

test("setGridSpec truncates a fractional value rather than storing one", () => {
  assert.equal(setGridSpec(config(), { cols: 5.9 }).profiles?.[0]?.grid?.cols, 5);
});

test("setGridSpec clamps a negative gap to zero", () => {
  const grid = setGridSpec(config(), { gapY: -4 }).profiles?.[0]?.grid;
  assert.ok(grid);
  assert.equal("gapY" in grid, false);
});

test("setGridSpec refuses, unchanged, when there is no profile", () => {
  const empty: WireConfig = { schema: 2, rev: 1, profiles: [] };
  assert.equal(setGridSpec(empty, { cols: 8 }), empty);
});
