import assert from "node:assert/strict";
import { test } from "node:test";

import { fillPlaceholders, isBareName, literal, quoteName } from "../src/modules/config/lib/condition.lib.ts";

test("a bare name is one the agent reads as a variable, not a condition", () => {
  assert.ok(isBareName("obs.recording"));
  assert.ok(isBareName("  obs.recording "));
  assert.ok(isBareName("my-plugin.on"), "a hyphen, as a whole hole reads it");
  assert.ok(isBareName("obs.input.Mic/Aux.muted"));
  assert.ok(!isBareName("obs.input.Mic Aux.muted"));
  assert.ok(isBareName("obs.input.Mic Aux.muted", ["obs.input.Mic Aux.muted"]), "a known name, whatever it holds");
  assert.ok(!isBareName("obs.preview_scene != obs.scene"));
  assert.ok(!isBareName("not obs.recording"));
  assert.ok(!isBareName("true"));
  assert.ok(!isBareName(""));
});

test("a name is quoted only when the agent would not read it bare", () => {
  assert.equal(quoteName("obs.scene"), "obs.scene");
  assert.equal(quoteName("my-plugin.on"), "`my-plugin.on`");
  assert.equal(quoteName("obs.input.Mic/Aux.muted"), "`obs.input.Mic/Aux.muted`");
  assert.equal(quoteName("and"), "`and`");
});

test("a value is always written as quoted text", () => {
  assert.equal(literal("Ending Stream"), '"Ending Stream"');
  assert.equal(literal('say "hi" \\ bye'), '"say \\"hi\\" \\\\ bye"');
  assert.equal(literal("0x10"), '"0x10"');
});

test("placeholders become a value or part of a name, never text inside quotes", () => {
  const args = { scene: "Ending Stream", input: "Mic/Aux", id: "desk" };
  assert.equal(fillPlaceholders("obs.scene == {{scene}}", args), 'obs.scene == "Ending Stream"');
  assert.equal(fillPlaceholders("obs.input.{{input}}.muted", args), "`obs.input.Mic/Aux.muted`");
  assert.equal(fillPlaceholders("lights.{{id}}.on and true", args), "lights.desk.on and true");
  assert.equal(fillPlaceholders("`obs.input.{{input}}.muted`", args), "`obs.input.Mic/Aux.muted`");
  assert.equal(fillPlaceholders('obs.scene == "{{scene}}"', args), null);
  assert.equal(fillPlaceholders("obs.scene == {{missing}}", args), null);
  assert.equal(fillPlaceholders("obs.scene == {{scene}}", { scene: " " }), null);
  assert.equal(fillPlaceholders("obs.studio_mode and obs.preview_scene != obs.scene", {}), "obs.studio_mode and obs.preview_scene != obs.scene");
  assert.equal(fillPlaceholders("a {{ }} b", {}), "a {{ }} b");
});
