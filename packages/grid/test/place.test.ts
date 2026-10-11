import assert from "node:assert/strict";
import { test } from "node:test";

import { orientPage, placePage, type GridSpec, type PlacedWidget } from "../src/index.ts";

// Landscape 6 across × 4 down; its own portrait 3 across × 9 down.
const landscape: GridSpec = { cols: 6, rows: 4, gapX: 8, gapY: 8, usableW: 1280, usableH: 800 };
const portrait: GridSpec = { cols: 3, rows: 9, gapX: 8, gapY: 8, usableW: 800, usableH: 1280 };

const at = (id: string, col: number, row: number, w = 1, h = 1) => ({ id, col, row, w, h });

test("landscape is the authored layout", () => {
  const widgets: PlacedWidget[] = [{ span: at("a", 5, 3) }];
  assert.deepEqual(placePage(landscape, portrait, widgets, "landscape").widgets, [{ ...at("a", 5, 3), assumed: false }]);
});

test("no portrait grid and nothing pinned is the old transpose", () => {
  const widgets: PlacedWidget[] = [{ span: at("a", 2, 1, 2, 1) }, { span: at("b", 5, 3) }];
  const placed = placePage(landscape, null, widgets, "portrait");
  const old = orientPage(landscape, widgets.map((widget) => widget.span), "portrait");
  assert.deepEqual(placed.spec, old.spec);
  assert.deepEqual(placed.widgets.map(({ assumed: _, ...span }) => span), old.widgets);
  assert.ok(placed.widgets.every((span) => span.assumed));
});

test("a turned span that fits its own portrait grid stays where turning puts it", () => {
  const placed = placePage(landscape, portrait, [{ span: at("a", 5, 2) }], "portrait");
  assert.deepEqual(placed.widgets, [{ ...at("a", 2, 5), assumed: true }]);
});

test("a turned span that falls outside the portrait grid takes the first free cell", () => {
  // Landscape row 3 turns into portrait column 3 — past a 3-column grid.
  const placed = placePage(landscape, portrait, [{ span: at("x", 0, 0) }, { span: at("a", 0, 3) }], "portrait");
  assert.deepEqual(placed.widgets, [
    { ...at("x", 0, 0), assumed: true },
    { ...at("a", 1, 0), assumed: true },
  ]);
});

test("a pinned position is kept, and assumed widgets go around it", () => {
  const widgets: PlacedWidget[] = [{ span: at("a", 0, 0) }, { span: at("b", 1, 0), portrait: at("b", 0, 0) }];
  const placed = placePage(landscape, portrait, widgets, "portrait");
  assert.deepEqual(placed.widgets, [
    { ...at("a", 1, 0), assumed: true },
    { ...at("b", 0, 0), assumed: false },
  ]);
});

test("a pin that no longer fits the grid falls back to an assumed position", () => {
  const widgets: PlacedWidget[] = [{ span: at("a", 0, 0), portrait: at("a", 7, 7) }];
  assert.deepEqual(placePage(landscape, portrait, widgets, "portrait").widgets, [{ ...at("a", 0, 0), assumed: true }]);
});

test("a span too wide for the portrait grid is narrowed to fit", () => {
  const placed = placePage(landscape, { ...portrait, cols: 2, rows: 2 }, [{ span: at("a", 0, 0, 1, 4) }], "portrait");
  assert.deepEqual(placed.widgets, [{ ...at("a", 0, 0, 2, 1), assumed: true }]);
});

test("a widget that fits nowhere is left out", () => {
  const tiny = { ...portrait, cols: 1, rows: 1 };
  const placed = placePage(landscape, tiny, [{ span: at("a", 0, 0) }, { span: at("b", 1, 0) }], "portrait");
  assert.deepEqual(placed.widgets.map((span) => span.id), ["a"]);
});
