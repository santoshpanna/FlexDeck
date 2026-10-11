import assert from "node:assert/strict";
import { test } from "node:test";

import { canvasGeometry, moveBox } from "../src/modules/config/lib/canvas.lib.ts";

test("a dragged box snaps, stays on the key, and never shrinks to nothing", () => {
  const box = { x: 0.1, y: 0.1, w: 0.3, h: 0.2 };
  assert.deepEqual(moveBox(box, 0.013, 0.031, false, false), { x: 0.125, y: 0.125, w: 0.3, h: 0.2 }, "snapped to 2.5%");
  assert.deepEqual(moveBox(box, 0.013, 0, false, true), { x: 0.113, y: 0.1, w: 0.3, h: 0.2 }, "Alt: where it was let go");
  assert.deepEqual(moveBox(box, 5, 5, false, false), { x: 0.7, y: 0.8, w: 0.3, h: 0.2 }, "kept on the key");
  assert.deepEqual(moveBox(box, -1, -1, true, false), { x: 0.1, y: 0.1, w: 0.04, h: 0.04 }, "never smaller than 4%");
  assert.deepEqual(moveBox(box, 2, 2, true, false), { x: 0.1, y: 0.1, w: 0.9, h: 0.9 }, "never past the edge");
});

test("the canvas is the key at tablet size, the face above its strip", () => {
  const widget = { id: "w", type: "WIDGET_TYPE_BUTTON" as const, col: 0, row: 0, w: 3, h: 1, face: { zones: [{ name: "a" }], elements: [{ template: "x", box: { x: 0.5, y: 0, w: 0.5, h: 1 } }] } };
  const { w, h, faceH, rects } = canvasGeometry(widget, widget.face);
  assert.equal(w, 148 * 3 + 24);
  assert.equal(h, 148);
  assert.ok(faceH < h);
  assert.deepEqual(rects[0], { x: 234, y: 0, w: 234, h: faceH });
});
