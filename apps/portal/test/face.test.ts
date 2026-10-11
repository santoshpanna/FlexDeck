import assert from "node:assert/strict";
import { test } from "node:test";

import type { WireWidget } from "../src/services/api/api.ts";
import { drawableColor, FACE_SWATCHES, faceColors, INK_SWATCHES } from "../src/modules/config/lib/face.lib.ts";

function keyWith(props: WireWidget["props"]): WireWidget {
  return { id: "w1", type: "WIDGET_TYPE_BUTTON", props } as WireWidget;
}

test("a set background is drawn solid and the ink follows the text colour", () => {
  assert.deepEqual(faceColors(keyWith({ bgColor: "#1E3A8A", fgColor: "#ffffff" }), false), {
    background: "#1e3a8a",
    ink: "#ffffff",
  });
});

test("while lit the lit face wins over the background, as on the tablet", () => {
  assert.equal(faceColors(keyWith({ bgColor: "#1e3a8a" }), true).background, null);
});

test("a colour the tablet cannot parse is treated as unset", () => {
  for (const bad of ["red", "#fff", "#1e3a8a80", "", undefined]) {
    assert.equal(drawableColor(bad), null, String(bad));
  }
  assert.deepEqual(faceColors(keyWith({ bgColor: "blue" }), false), { background: null, ink: null });
});

test("every swatch is something the tablet can draw", () => {
  for (const swatch of [...FACE_SWATCHES, ...INK_SWATCHES]) {
    assert.equal(drawableColor(swatch.color), swatch.color, swatch.name);
  }
});
