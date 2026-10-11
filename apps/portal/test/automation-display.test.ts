import assert from "node:assert/strict";
import { test } from "node:test";

import { describeBlock, describeTrigger } from "../src/modules/config/components/AutomationEditor/describe.lib.ts";
import type { Automation } from "../src/services/api/api.ts";

/**
 * Human-readable one-liners for automations and their blocks.
 *
 * The list view shows these next to the name, so a screen reader or a glance
 * tells the user what the automation does. The editor renders the full shape;
 * these one-liners are the secondary surface.
 */

test("a variables trigger lists its variable names", () => {
  const text = describeTrigger({ on: "variables", names: ["volume", "device"] });
  assert.equal(text, "on volume, device change");
});

test("a variables trigger with no names says so", () => {
  assert.equal(describeTrigger({ on: "variables", names: [] }), "on any variable change");
});

test("a widget trigger names the widget", () => {
  assert.equal(describeTrigger({ on: "widget", widgetId: "w-1" }), "on widget w-1 press");
});

test("a widget trigger with no id marks it as unset", () => {
  assert.equal(describeTrigger({ on: "widget", widgetId: "" }), "on widget (unset) press");
});

test("a manual trigger is one word", () => {
  assert.equal(describeTrigger({ on: "manual" }), "manual");
});

test("if/then describes its condition and step count", () => {
  const text = describeBlock({
    block: "if",
    condition: { kind: "compare", left: { variable: "v" }, op: "eq", right: { number: 1 } },
    then: [{ block: "wait", ms: { number: 10 } }, { block: "wait", ms: { number: 20 } }],
  });
  assert.equal(text, "if $v == 1 → 2 steps");
});

test("repeat describes its loop count and body", () => {
  const text = describeBlock({
    block: "repeat",
    times: { number: 3 },
    body: [{ block: "wait", ms: { number: 1 } }],
  });
  assert.equal(text, "repeat 3× (1 step)");
});

test("wait describes its delay", () => {
  assert.equal(describeBlock({ block: "wait", ms: { number: 500 } }), "wait 500ms");
});

test("setVariable describes its target and value", () => {
  assert.equal(
    describeBlock({ block: "setVariable", name: "device", value: { text: "speakers" } }),
    'set device = "speakers"',
  );
});

test("doAction describes its plugin, action and args", () => {
  const text = describeBlock({
    block: "doAction",
    plugin: "core",
    action: "press",
    args: { combo: { variable: "shortcut" } },
  });
  assert.equal(text, "do core:press (combo=$shortcut)");
});

test("doAction with no args has no trailing parenthesised list", () => {
  const text = describeBlock({
    block: "doAction",
    plugin: "core",
    action: "noop",
    args: {},
  });
  assert.equal(text, "do core:noop");
});

test("describeBlock on a future-version block falls through silently", () => {
  // The discriminated union makes this unreachable at compile time; the runtime
  // path returns undefined for an unknown `block` discriminant (a future schema
  // version may add one). The editor renders nothing rather than crashing, which
  // is the contract: unknown shapes are simply not displayed.
  const unknown = { block: "fly" } as unknown as Parameters<typeof describeBlock>[0];
  assert.equal(describeBlock(unknown), undefined);
});

test("an Automation with the four building-block kinds has four readable steps", () => {
  const automation: Automation = {
    id: "a1",
    name: "example",
    enabled: true,
    trigger: { on: "variables", names: ["v"] },
    body: [
      { block: "wait", ms: { number: 100 } },
      { block: "setVariable", name: "x", value: { number: 1 } },
      { block: "if", condition: { kind: "compare", left: { variable: "x" }, op: "eq", right: { number: 1 } }, then: [] },
      { block: "doAction", plugin: "core", action: "noop", args: {} },
    ],
  };
  for (const step of automation.body) {
    assert.ok(describeBlock(step).length > 0);
  }
  assert.equal(describeTrigger(automation.trigger), "on v change");
});