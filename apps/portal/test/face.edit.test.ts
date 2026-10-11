import assert from "node:assert/strict";
import { test } from "node:test";

import type { WireConfig, WireWidget } from "../src/services/api/api.ts";
import {
  addElement,
  addLook,
  clearLookFace,
  editFace,
  lookSourceOf,
  moveElement,
  nextLookId,
  removeLook,
  setBackground,
  setCustomFace,
  setLayout,
  setLookSource,
  setPageBackground,
  starterElement,
  updateLook,
} from "../src/modules/config/lib/face.edit.lib.ts";

function deck(widget: WireWidget): WireConfig {
  return { schema: 3, rev: 1, profiles: [{ id: "p", pages: [{ id: "home", widgets: [widget] }] }] };
}

const widgetOf = (config: WireConfig): WireWidget => config.profiles![0]!.pages![0]!.widgets![0]!;

test("a custom face starts as what the props drew, and the deck becomes schema 4", () => {
  const config = setCustomFace(deck({ id: "w", props: { label: "Mute", icon: "mic", bgColor: "#1e40af" } }), "w", true);
  assert.equal(config.schema, 4);
  assert.deepEqual(widgetOf(config).face, {
    background: { color: "#1e40af" },
    elements: [{ slot: "SLOT_ICON", kind: "ELEMENT_KIND_ICON", template: "mic" }, { template: "Mute" }],
  });
  const back = setCustomFace(config, "w", false);
  assert.equal(widgetOf(back).face, undefined);
});

test("editing a face changes only the face being edited", () => {
  let config = deck({ id: "w", props: { label: "CPU" } });
  config = editFace(config, "w", "", (face) => addElement(setLayout(face, "FACE_LAYOUT_DIAL"), starterElement("ELEMENT_KIND_GAUGE", "FACE_LAYOUT_DIAL")));
  const face = widgetOf(config).face!;
  assert.equal(face.layout, "FACE_LAYOUT_DIAL");
  assert.equal(face.elements?.[1]?.kind, "ELEMENT_KIND_GAUGE");
  assert.equal(setLayout(face, "FACE_LAYOUT_ICON_LABEL").layout, undefined, "the default is absent, as on the wire");
  assert.deepEqual(moveElement(face, 0, 1).elements?.map((e) => e.kind), ["ELEMENT_KIND_GAUGE", undefined]);
  assert.equal(moveElement(face, 0, -1), face, "nowhere to go: unchanged");
});

test("a background drops what is unset", () => {
  assert.deepEqual(setBackground({}, { color: "#000000", image: "", dim: 0, transparent: false }), { background: { color: "#000000" } });
  assert.deepEqual(setBackground({ background: { color: "#000000" } }, {}), {});
});

test("looks: add, rename, give one its own face, remove it and the cases that showed it", () => {
  let config = deck({ id: "w", props: { label: "CPU" } });
  const first = nextLookId(widgetOf(config));
  config = addLook(config, "w", { id: first, label: "Hot", bgColor: "#7f1d1d" });
  assert.equal(nextLookId(widgetOf(config)), "look-2");
  config = addLook(config, "w", { id: "look-2" });
  config = setLookSource(config, "w", {
    kind: "variable",
    variable: "system.cpu.usage",
    cases: [{ op: "COMPARE_OP_GT", value: "80", state: first }, { op: "COMPARE_OP_EQ", value: "0", state: "look-2" }],
    otherwise: "look-2",
  });
  assert.deepEqual(widgetOf(config).states?.variable?.cases, [
    { op: "COMPARE_OP_GT", value: "80", state: first },
    { value: "0", state: "look-2" },
  ]);
  assert.equal(lookSourceOf(widgetOf(config)).kind, "variable");

  config = updateLook(config, "w", first, { label: "  " });
  assert.equal(widgetOf(config).states?.looks?.[0]?.label, undefined, "blank is absent");

  config = editFace(config, "w", first, (face) => addElement(face, { slot: "SLOT_CAPTION", template: "HOT" }));
  const own = widgetOf(config).states?.looks?.[0]?.face;
  assert.equal(own?.background?.color, "#7f1d1d", "it starts as what the look showed");
  assert.equal(own?.elements?.at(-1)?.template, "HOT");
  config = clearLookFace(config, "w", first);
  assert.equal(widgetOf(config).states?.looks?.[0]?.face, undefined);

  config = removeLook(config, "w", "look-2");
  assert.deepEqual(widgetOf(config).states?.variable?.cases?.map((c) => c.state), [first]);
  assert.equal(widgetOf(config).states?.variable?.otherwise, "");
  config = removeLook(config, "w", first);
  assert.equal(widgetOf(config).states, undefined, "no looks, no rule");
});

test("a cycle keeps the looks and drops the variable", () => {
  let config = addLook(deck({ id: "w" }), "w", { id: "a" });
  config = setLookSource(config, "w", { kind: "cycle" });
  assert.deepEqual(widgetOf(config).states, { looks: [{ id: "a" }], cycle: {} });
});

test("a page background is set and cleared, and an unknown page is no change", () => {
  const config = deck({ id: "w" });
  const set = setPageBackground(config, "home", { image: "img:ab", dim: 30 });
  assert.deepEqual(set.profiles![0]!.pages![0]!.background, { image: "img:ab", dim: 30 });
  assert.equal(setPageBackground(set, "home", null).profiles![0]!.pages![0]!.background, undefined);
  assert.equal(setPageBackground(config, "nowhere", { color: "#000000" }), config);
});

test("a key's own run of a source keeps only the settings it changes", async () => {
  const { setKeySource } = await import("../src/modules/config/lib/face.edit.lib.ts");
  let config = deck({ id: "w" });
  config = setKeySource(config, "w", { source: "weather", args: { lat: "35.68", lon: "" } });
  assert.deepEqual(widgetOf(config).sources, [{ source: "weather", args: { lat: "35.68" } }]);
  assert.equal(config.schema, 4);
  config = setKeySource(config, "w", null);
  assert.equal(widgetOf(config).sources, undefined);
});

test("screen care is set on the page's profile, and cleared when all off", async () => {
  const { setScreenCare } = await import("../src/modules/config/lib/face.edit.lib.ts");
  let config = deck({ id: "w" });
  config = setScreenCare(config, "home", { shift: true, dim: true, dimFrom: 1320, dimTo: 420 });
  assert.deepEqual(config.profiles![0]!.screen, { shift: true, dim: true, dimFrom: 1320, dimTo: 420 });
  assert.equal(config.schema, 4);
  config = setScreenCare(config, "home", { shift: false, dim: false });
  assert.equal(config.profiles![0]!.screen, undefined);
  assert.equal(setScreenCare(config, "nowhere", { shift: true }), config);
});

test("a zone's flows are its own, follow a rename and leave with it", async () => {
  const { MUSIC_ZONES, renameZone, setZones } = await import("../src/modules/config/lib/face.edit.lib.ts");
  const { gestureBlocks, setGestureBlocks } = await import("../src/modules/config/lib/flow.lib.ts");
  const tap = "GESTURE_SHORT_PRESS" as const;
  const hotkey = (keys: string) => ({ block: "doAction", plugin: "keyboard", action: "hotkey", args: { keys: { text: keys } } }) as never;
  let config = deck({ id: "w", type: "WIDGET_TYPE_BUTTON", col: 0, row: 0, w: 3, h: 1, face: setZones({}, [...MUSIC_ZONES]) });
  config = setGestureBlocks(config, "w", tap, [hotkey("open")]);
  config = setGestureBlocks(config, "w", tap, [hotkey("next")], "next");
  assert.equal(gestureBlocks(widgetOf(config), tap).length, 1, "the key's own");
  assert.equal(gestureBlocks(widgetOf(config), tap, "next").length, 1);
  // Clearing the key's tap leaves the zone's alone.
  config = setGestureBlocks(config, "w", tap, []);
  assert.deepEqual(widgetOf(config).flows?.map((f) => f.zone), ["next"]);

  config = renameZone(config, "w", "next", "skip");
  assert.deepEqual(widgetOf(config).face?.zones?.map((z) => z.name), ["prev", "play", "skip"]);
  assert.equal(gestureBlocks(widgetOf(config), tap, "skip").length, 1);

  config = renameZone(config, "w", "skip", null);
  assert.deepEqual(widgetOf(config).face?.zones?.map((z) => z.name), ["prev", "play"]);
  assert.equal(widgetOf(config).flows, undefined);
});
