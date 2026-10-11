import assert from "node:assert/strict";
import { test } from "node:test";

import type { WireConfig, WireWidget } from "../src/services/api/api.ts";
import { setKeyFill } from "../src/modules/config/lib/face.edit.lib.ts";
import { backgroundCss } from "../src/modules/config/lib/face.lib.ts";
import { fillCss, fillLabel, fillOf, hexToHsv, hsvToHex, parseHex, switchMode, toBackground, type Fill } from "../src/modules/config/lib/fill.lib.ts";

test("each fill survives a trip through a background", () => {
  const fills: Fill[] = [
    { kind: "default" },
    { kind: "solid", color: "#9a3412" },
    { kind: "gradient", from: "#9a3412", to: "#1f1f23", angle: 180, radial: false },
    { kind: "gradient", from: "#9a3412", to: "#1f1f23", angle: 135, radial: false },
    { kind: "gradient", from: "#9a3412", to: "#1f1f23", angle: 0, radial: false },
    { kind: "gradient", from: "#9a3412", to: "#1f1f23", angle: 180, radial: true },
    { kind: "image", image: "img:abc", fit: "IMAGE_FIT_CONTAIN", dim: 40 },
    { kind: "image", image: "{{media.current.art}}", fit: "IMAGE_FIT_COVER", dim: 0 },
    { kind: "transparent" },
  ];
  for (const fill of fills) {
    assert.deepEqual(fillOf(toBackground(fill, undefined) ?? undefined), fill, JSON.stringify(fill));
  }
});

test("defaults are left out, as the wire leaves them out", () => {
  assert.equal(toBackground({ kind: "default" }, undefined), null);
  assert.deepEqual(toBackground({ kind: "gradient", from: "#111111", to: "#222222", angle: 180, radial: false }, undefined), { color: "#111111", colorTo: "#222222" });
  // 0° is up; written as 360 so it is not read back as unset (180).
  assert.equal(toBackground({ kind: "gradient", from: "#111111", to: "#222222", angle: 0, radial: false }, undefined)?.angle, 360);
  assert.deepEqual(toBackground({ kind: "image", image: "img:a", fit: "IMAGE_FIT_COVER", dim: 0 }, undefined), { image: "img:a" });
});

test("changing the fill keeps the graph drawn behind and drops the old fill", () => {
  const before = { color: "#111111", colorTo: "#222222", angle: 90, graph: "system.cpu.usage", graphColor: "#4ade80" };
  assert.deepEqual(toBackground({ kind: "image", image: "img:a", fit: "IMAGE_FIT_COVER", dim: 0 }, before), { image: "img:a", graph: "system.cpu.usage", graphColor: "#4ade80" });
  assert.deepEqual(toBackground({ kind: "default" }, before), { graph: "system.cpu.usage", graphColor: "#4ade80" });
});

test("a picture wins over a colour when reading a background", () => {
  assert.equal(fillOf({ color: "#111111", image: "img:a" }).kind, "image");
  assert.equal(fillOf({ color: "#111111", transparent: true }).kind, "transparent");
  assert.equal(fillOf({ color: "not a colour" }).kind, "default");
});

test("switching modes carries the colour over", () => {
  const solid: Fill = { kind: "solid", color: "#115e59" };
  const gradient = switchMode(solid, "gradient");
  assert.equal(gradient.kind === "gradient" && gradient.from, "#115e59");
  assert.deepEqual(switchMode(gradient, "solid"), solid);
});

test("fills as CSS, the way the tablet paints them", () => {
  assert.equal(fillCss({ kind: "solid", color: "#111111" }), "#111111");
  assert.equal(fillCss({ kind: "gradient", from: "#111111", to: "#222222", angle: 135, radial: false }), "linear-gradient(135deg, #111111, #222222)");
  assert.equal(fillCss({ kind: "gradient", from: "#111111", to: "#222222", angle: 0, radial: false }), "linear-gradient(0deg, #111111, #222222)");
  assert.equal(fillCss({ kind: "gradient", from: "#111111", to: "#222222", angle: 180, radial: true }), "radial-gradient(circle at center, #111111, #222222)");
  assert.equal(fillCss({ kind: "image", image: "img:a", fit: "IMAGE_FIT_COVER", dim: 0 }), null);
  assert.equal(backgroundCss({ color: "#111111", colorTo: "#222222" }), "linear-gradient(180deg, #111111, #222222)");
  assert.equal(backgroundCss({ color: "#111111", colorTo: "#222222", angle: 360 }), "linear-gradient(0deg, #111111, #222222)");
  assert.equal(backgroundCss({}), null);
});

test("labels say what the fill is", () => {
  assert.equal(fillLabel({ kind: "solid", color: "#9a3412" }), "Rust");
  assert.equal(fillLabel({ kind: "gradient", from: "#9a3412", to: "#123456", angle: 135, radial: false }), "Rust → #123456 135°");
});

test("hex and HSV round-trip", () => {
  for (const hex of ["#000000", "#ffffff", "#9a3412", "#115e59", "#4c1d95", "#facc15", "#ff0000", "#00ff00", "#0000ff"]) {
    assert.equal(hsvToHex(hexToHsv(hex)), hex);
  }
  assert.equal(parseHex("F60"), "#ff6600");
  assert.equal(parseHex(" #AbCdEf "), "#abcdef");
  assert.equal(parseHex("ggg"), null);
});

function deck(widget: WireWidget): WireConfig {
  return { schema: 3, rev: 1, profiles: [{ id: "p", pages: [{ id: "home", widgets: [widget] }] }] };
}
const widgetOf = (config: WireConfig): WireWidget => config.profiles![0]!.pages![0]!.widgets![0]!;

test("a simple key keeps a plain colour in its props", () => {
  const key = { id: "w", props: { label: "Mute", fgColor: "#111111" } };
  assert.deepEqual(widgetOf(setKeyFill(deck(key), "w", { kind: "solid", color: "#9a3412" })), { id: "w", props: { label: "Mute", fgColor: "#111111", bgColor: "#9a3412" } });
  assert.deepEqual(widgetOf(setKeyFill(deck({ ...key, props: { ...key.props, bgColor: "#9a3412" } }), "w", { kind: "default" })), key);
});

test("a fade on a simple key gives it a face that otherwise draws the same", () => {
  const widget = widgetOf(setKeyFill(deck({ id: "w", props: { label: "Mute", icon: "mic", bgColor: "#9a3412" } }), "w", { kind: "gradient", from: "#9a3412", to: "#1f1f23", angle: 135, radial: false }));
  assert.ok(widget.face);
  assert.deepEqual(widget.face.background, { color: "#9a3412", colorTo: "#1f1f23", angle: 135 });
  const templates = (widget.face.elements ?? []).map((element) => element.template);
  assert.ok(templates.includes("Mute"));
  assert.ok(templates.includes("mic"));
});

test("a key with a face has its face's background filled, keeping its graph", () => {
  const key: WireWidget = { id: "w", face: { layout: "FACE_LAYOUT_VALUE", background: { color: "#111111", graph: "system.cpu.usage" } } };
  const widget = widgetOf(setKeyFill(deck(key), "w", { kind: "image", image: "img:a", fit: "IMAGE_FIT_COVER", dim: 20 }));
  assert.deepEqual(widget.face?.background, { image: "img:a", dim: 20, graph: "system.cpu.usage" });
});
