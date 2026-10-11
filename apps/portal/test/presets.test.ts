import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { PRESETS_FILE, presetsJson } from "../scripts/gen-presets.ts";
import type { WireConfig } from "../src/services/api/api.ts";
import { PRESETS, STARTER_LAYOUTS, placeLayout, placePreset } from "../src/modules/config/lib/presets.lib.ts";
import { STARTERS } from "../src/modules/config/lib/sources.lib.ts";

function deck(cols = 5, rows = 3): WireConfig {
  return { schema: 3, profiles: [{ id: "p", grid: { cols, rows, usableW: 1280, usableH: 800 }, pages: [{ id: "home", widgets: [] }] }] };
}

test("the committed presets are the gallery's, so the agent checks what ships", () => {
  assert.equal(readFileSync(PRESETS_FILE, "utf8"), presetsJson(), "run pnpm gen:presets");
});

test("every preset has a unique id, and every source it needs is a starter", () => {
  assert.equal(new Set(PRESETS.map((p) => p.id)).size, PRESETS.length);
  for (const preset of PRESETS) {
    if (preset.needs) assert.ok(STARTERS.some((s) => s.label === preset.needs!.starter), `${preset.id} needs ${preset.needs.starter}`);
  }
  for (const layout of STARTER_LAYOUTS) {
    for (const id of layout.presets) assert.ok(PRESETS.some((p) => p.id === id), `${layout.id}: ${id}`);
  }
});

test("a preset lands at its size where it fits, smaller where it does not, and the deck becomes schema 4", () => {
  const weather = PRESETS.find((p) => p.id === "weather")!;
  const placed = placePreset(deck(), "home", weather)!;
  const widget = placed.config.profiles![0]!.pages![0]!.widgets![0]!;
  assert.deepEqual([widget.col, widget.row, widget.w, widget.h], [0, 0, 2, 1]);
  assert.equal(placed.config.schema, 4);
  assert.equal(widget.props?.label, "Weather", "named, so the editor lists it");
  const narrow = placePreset(deck(1, 1), "home", weather)!;
  assert.equal(narrow.config.profiles![0]!.pages![0]!.widgets![0]!.w, 1);
  assert.equal(placePreset(narrow.config, "home", weather), null, "no room");
});

test("a starter layout places what fits and lists the sources it needs", () => {
  const { config, placed, needs } = placeLayout(deck(), "home", "dashboard");
  assert.ok(placed >= 5, `placed ${placed}`);
  assert.equal(config.profiles![0]!.pages![0]!.widgets!.length, placed);
  assert.deepEqual(needs.map((n) => n?.id), ["weather"]);
});
