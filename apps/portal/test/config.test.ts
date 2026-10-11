import assert from "node:assert/strict";
import { test } from "node:test";

import { layoutPage } from "@flexdeck/grid";

import {
  gridSpecOf,
  labelOf,
  pageOf,
  profileOf,
  widgetSpansOf,
} from "../src/modules/config/lib/config.lib.ts";

test("an absent grid field means zero, not a default", () => {
  // This is the shape the agent actually sends for a grid with no gaps: proto3
  // JSON omits default values, so `gapX: 0` arrives as an absent key.
  const spec = gridSpecOf({ id: "tab1", grid: { cols: 4, rows: 3 } });

  assert.equal(spec.cols, 4);
  assert.equal(spec.gapX, 0, "an absent gap is a zero gap, not an invented one");
  assert.equal(spec.usableW, 0);
});

test("a grid with no gaps still lays out correctly", () => {
  // Guards the consequence rather than the value: 400px over 4 columns with no
  // gaps must be four 100px cells, not four cells separated by invented gaps.
  const spec = gridSpecOf({ id: "tab1", grid: { cols: 4, rows: 3, usableW: 400, usableH: 300 } });
  const rects = layoutPage(spec, [{ id: "a", col: 1, row: 0, w: 1, h: 1 }]);

  assert.deepEqual(rects["a"], { x: 100, y: 0, w: 100, h: 100 });
});

test("a profile with no grid at all is all zeroes rather than a crash", () => {
  const spec = gridSpecOf({ id: "tab1" });

  assert.deepEqual(spec, { cols: 0, rows: 0, gapX: 0, gapY: 0, usableW: 0, usableH: 0 });
  // The grid algorithm treats zero cells as empty, which is what the Rust and
  // Kotlin implementations do too.
  assert.deepEqual(layoutPage(spec, []), {});
});

test("profiles, pages and widgets are read defensively", () => {
  assert.equal(profileOf(null), null);
  assert.equal(profileOf({}), null);
  assert.equal(pageOf(null), null);
  assert.deepEqual(widgetSpansOf(null), []);
});

test("a widget with no span is clamped to at least one cell", () => {
  // A zero-sized widget would render as nothing at all, which looks like a bug
  // in the portal rather than a gap in the configuration.
  const spans = widgetSpansOf({
    id: "home",
    widgets: [{ id: "mute" }, { id: "zero", w: 0, h: 0 }],
  });

  assert.deepEqual(spans, [
    { id: "mute", col: 0, row: 0, w: 1, h: 1 },
    { id: "zero", col: 0, row: 0, w: 1, h: 1 },
  ]);
});

test("a labelled widget shows its label", () => {
  assert.equal(labelOf({ id: "mute", props: { label: "Mute" } }), "Mute");
});

test("an unlabelled widget falls back to its id", () => {
  // Showing nothing would be indistinguishable from a rendering failure.
  assert.equal(labelOf({ id: "mute" }), "mute");
  assert.equal(labelOf({ id: "mute", props: { label: "   " } }), "mute");
});
