import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { PRESETS_FILE, presetsJson } from "../scripts/gen-presets.ts";
import type { WireConfig, WireWidget } from "../src/services/api/api.ts";
import { PRESETS, applyPresetLook, applyWholePreset, isBlankKey, isLookPreset, placePreset, presetFits, presetHalves, sharedReads, suggestedActions, type Preset } from "../src/modules/config/lib/presets.lib.ts";
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


const preset = (id: string): Preset => PRESETS.find((p) => p.id === id)!;

/** A deck with a scene key at the top left, lit while its scene is live. */
function sceneDeck(extra: WireWidget[] = []): WireConfig {
  const config = deck(5, 3);
  config.profiles![0]!.pages![0]!.widgets = [
    {
      id: "w1",
      type: "WIDGET_TYPE_BUTTON",
      col: 0,
      row: 0,
      w: 1,
      h: 1,
      props: { label: "CS2" },
      flows: [{ trigger: { gesture: "GESTURE_SHORT_PRESS" }, blocks: [{ block: "doAction", plugin: "obs", action: "set_scene", args: { scene: { text: "CS2" } } }] }],
      states: { looks: [{ id: "off" }, { id: "on" }], variable: { variable: "obs.scene", cases: [{ value: "CS2", state: "on" }], otherwise: "off" } },
    },
    ...extra,
  ];
  return config;
}
const keyOf = (config: WireConfig, id = "w1") => config.profiles![0]!.pages![0]!.widgets!.find((w) => w.id === id)!;

test("a preset says which halves it fills", () => {
  assert.deepEqual(presetHalves(preset("cpu-gauge")), { live: true, gestures: [] });
  assert.deepEqual(presetHalves(preset("pomodoro")), { live: true, gestures: ["GESTURE_SHORT_PRESS", "GESTURE_LONG_PRESS"] });
  assert.deepEqual(presetHalves(preset("media-play_pause")), { live: false, gestures: ["GESTURE_SHORT_PRESS"] });
});

test("looks that can go on another key leave out strips of buttons and states", () => {
  assert.ok(isLookPreset(preset("cpu-gauge")));
  assert.ok(isLookPreset(preset("media-next")), "an icon is a look");
  assert.ok(!isLookPreset(preset("now-playing-controls")), "its strip belongs to its flows");
  assert.ok(!isLookPreset(preset("mic")), "its look is states, which would replace the key's own");
});

test("a look goes on a key and keeps what it does: its press, its scene light, its name", () => {
  const before = keyOf(sceneDeck());
  const after = keyOf(applyPresetLook(sceneDeck(), "w1", preset("cpu-gauge")));
  assert.deepEqual(after.face, preset("cpu-gauge").widget.face);
  assert.deepEqual(after.flows, before.flows);
  assert.deepEqual(after.states, before.states);
  assert.equal(after.props?.label, "CS2");
});

test("a look reads the shared source, not a run of its own; an icon-only look sets just the icon", () => {
  const weather = keyOf(applyPresetLook(sceneDeck(), "w1", preset("weather")));
  assert.equal(weather.sources, undefined);
  assert.ok(JSON.stringify(weather.face).includes("src.weather.temp"));
  const plugin = sharedReads({ sources: [{ source: "crypto" }], face: { elements: [{ template: "{{ this.price | currency }}" }], rules: [{ when: "this.change < 0" }] } });
  assert.equal(plugin.sources, undefined);
  assert.equal(plugin.face?.elements?.[0]?.template, "{{ src.crypto.price | currency }}");
  assert.equal(plugin.face?.rules?.[0]?.when, "src.crypto.change < 0");
  const feed = sharedReads({ sources: [{ source: "spotify/now" }], props: { labelTemplate: "{{this.title}}" } });
  assert.deepEqual(feed.sources, [{ source: "spotify/now" }], "a plugin's feed is the key's own");
  assert.equal(feed.props?.labelTemplate, "{{this.title}}");
  const icon = keyOf(applyPresetLook(sceneDeck(), "w1", preset("media-next")));
  assert.equal(icon.face, undefined);
  assert.deepEqual(icon.props, { label: "CS2", icon: "skip-forward" });
});

test("a slider takes no look, and a look that is not one changes nothing", () => {
  const config = sceneDeck([{ id: "s", type: "WIDGET_TYPE_SLIDER", col: 4, row: 0, w: 1, h: 3 }]);
  assert.equal(applyPresetLook(config, "s", preset("cpu-gauge")), config);
  assert.equal(applyPresetLook(config, "w1", preset("mic")), config);
});

test("a key can grow to the look's size only where there is room", () => {
  assert.ok(presetFits(sceneDeck(), "w1", preset("network")), "2x1 with the next cell free");
  const blocked = sceneDeck([{ id: "w2", type: "WIDGET_TYPE_BUTTON", col: 1, row: 0, w: 1, h: 1 }]);
  assert.ok(!presetFits(blocked, "w1", preset("network")));
  assert.ok(!presetFits(sceneDeck(), "w1", preset("cpu-gauge")), "already its size");
});

test("a key that shows something is offered a press that fits, if the desktop has it", () => {
  const gauge: WireWidget = { id: "g", ...structuredClone(preset("cpu-gauge").widget) };
  const launcher = [{ plugin: "launcher", action: "open_app" }];
  assert.deepEqual(
    suggestedActions(gauge, launcher).map((s) => s.label),
    ["Open Task Manager"],
  );
  assert.deepEqual(suggestedActions(gauge, launcher)[0]!.block, { block: "doAction", plugin: "launcher", action: "open_app", args: { app: { text: "taskmgr" } } });
  assert.deepEqual(suggestedActions(gauge, []), [], "nothing the desktop cannot do");
  assert.deepEqual(suggestedActions({ id: "plain", props: { label: "Hi" } }, launcher), []);
});

test("a blank key is one with nothing on it yet", () => {
  assert.ok(isBlankKey({ id: "n", props: { label: "New button" } }));
  assert.ok(!isBlankKey(keyOf(sceneDeck())));
  assert.ok(!isBlankKey({ id: "n", props: { label: "Lights" } }));
});

test("a key made the whole preset takes its look and its press, keeps its place and a name it was given", () => {
  const blank: WireWidget = { id: "w2", type: "WIDGET_TYPE_BUTTON", col: 2, row: 1, w: 1, h: 1, props: { label: "New button" } };
  const made = keyOf(applyWholePreset(sceneDeck([blank]), "w2", preset("pomodoro")), "w2");
  assert.deepEqual(made.flows, preset("pomodoro").widget.flows);
  assert.deepEqual(made.face, preset("pomodoro").widget.face);
  assert.equal(made.props?.label, "Pomodoro");
  assert.deepEqual([made.col, made.row], [2, 1]);

  const replaced = keyOf(applyWholePreset(sceneDeck(), "w1", preset("mic")));
  assert.equal(replaced.props?.label, "CS2", "a name someone gave it stays");
  assert.equal(replaced.props?.icon, "mic");
  assert.deepEqual(replaced.flows, preset("mic").widget.flows);
  assert.deepEqual(replaced.states, preset("mic").widget.states);
});

test("a key made a wide preset grows to it when there is room", () => {
  const made = keyOf(applyWholePreset(sceneDeck(), "w1", preset("network")));
  assert.deepEqual([made.w, made.h], [2, 1]);
});
