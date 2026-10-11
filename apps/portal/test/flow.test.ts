import assert from "node:assert/strict";
import { test } from "node:test";

import type { Block, WireConfig, WireWidget } from "../src/services/api/api.ts";
import {
  conditionParts,
  duplicateBlock,
  emptyBlock,
  gestureBlocks,
  holdsOf,
  insertBlock,
  joinConditions,
  laneBlocks,
  literalValue,
  moveBlock,
  removeBlock,
  replaceBlock,
  setGestureBlocks,
  slotCounts,
  textValue,
  valueText,
} from "../src/modules/config/lib/flow.lib.ts";

const wait = (ms: number): Block => ({ block: "wait", ms: { number: ms } });

const nested: Block[] = [
  wait(1),
  {
    block: "if",
    condition: { kind: "compare", left: { variable: "x" }, op: "eq", right: { bool: true } },
    then: [wait(2), { block: "repeat", times: { number: 2 }, body: [wait(3)] }],
    otherwise: [wait(4)],
  },
];

test("laneBlocks walks into branches", () => {
  assert.deepEqual(laneBlocks(nested, []), nested);
  assert.deepEqual(laneBlocks(nested, [{ at: 1, branch: "then" }])[0], wait(2));
  assert.deepEqual(laneBlocks(nested, [{ at: 1, branch: "then" }, { at: 1, branch: "body" }]), [wait(3)]);
  assert.deepEqual(laneBlocks(nested, [{ at: 1, branch: "otherwise" }]), [wait(4)]);
  assert.deepEqual(laneBlocks(nested, [{ at: 0, branch: "then" }]), [], "a wait has no branches");
});

test("insert, remove, replace and move work at any depth without touching siblings", () => {
  const deep = [{ at: 1, branch: "then" as const }, { at: 1, branch: "body" as const }];
  const inserted = insertBlock(nested, deep, 0, wait(9));
  assert.deepEqual(laneBlocks(inserted, deep), [wait(9), wait(3)]);
  assert.deepEqual(laneBlocks(inserted, [{ at: 1, branch: "otherwise" }]), [wait(4)]);
  assert.deepEqual(nested[1], nested[1], "the input is not mutated");
  assert.deepEqual(laneBlocks(nested, deep), [wait(3)]);

  const removed = removeBlock(inserted, deep, 1);
  assert.deepEqual(laneBlocks(removed, deep), [wait(9)]);

  const replaced = replaceBlock(nested, [], 0, wait(7));
  assert.deepEqual(replaced[0], wait(7));

  const moved = moveBlock([wait(1), wait(2), wait(3)], [], 0, 2);
  assert.deepEqual(moved, [wait(2), wait(3), wait(1)]);
  assert.deepEqual(moveBlock([wait(1), wait(2)], [], 1, 99), [wait(1), wait(2)]);
});

test("emptying an else branch drops it from the wire", () => {
  const cleared = removeBlock(nested, [{ at: 1, branch: "otherwise" }], 0);
  assert.equal("otherwise" in (cleared[1] as object), false);
});

test("duplicate copies deeply", () => {
  const copied = duplicateBlock(nested, [], 1);
  assert.equal(copied.length, 3);
  assert.deepEqual(copied[1], copied[2]);
  assert.notEqual(copied[1], copied[2]);
});

test("every empty block is a valid kind", () => {
  for (const kind of ["if", "repeat", "wait", "setVariable", "doAction"] as const) assert.equal(emptyBlock(kind).block, kind);
});

test("values round-trip through text", () => {
  assert.deepEqual(textValue("{{obs.scene}}"), { variable: "obs.scene" });
  assert.deepEqual(textValue("{{input.value}}"), { input: "value" });
  assert.deepEqual(textValue("REC {{x}}"), { text: "REC {{x}}" });
  assert.deepEqual(literalValue("42"), { number: 42 });
  assert.deepEqual(literalValue("false"), { bool: false });
  for (const value of [{ variable: "a.b" }, { input: "value" }, { text: "hi" }] as const) assert.deepEqual(textValue(valueText(value)), value);
});

test("conditions split into parts and join back", () => {
  const one = { kind: "compare" as const, left: { variable: "a" }, op: "eq" as const, right: { bool: true } };
  assert.deepEqual(conditionParts(one), { mode: "all", parts: [one] });
  assert.deepEqual(joinConditions("all", [one]), one);
  assert.deepEqual(joinConditions("any", [one, one]), { kind: "any", conditions: [one, one] });
});

function configWith(widget: WireWidget): WireConfig {
  return {
    schema: 3,
    rev: 1,
    profiles: [{ id: "p", name: "p", grid: { cols: 2, rows: 2, gapX: 0, gapY: 0, usableW: 100, usableH: 100 }, pages: [{ id: "pg", name: "pg", widgets: [widget] }] }],
  } as WireConfig;
}

function widgetOf(config: WireConfig): WireWidget {
  return config.profiles![0]!.pages![0]!.widgets![0]!;
}

test("a gesture's flow is set without disturbing the others", () => {
  const start: WireWidget = {
    id: "w",
    flows: [
      { trigger: { gesture: "GESTURE_SHORT_PRESS" }, blocks: [wait(1)] },
      { name: "on scene", trigger: {}, blocks: [wait(5)] },
    ],
  };
  const next = widgetOf(setGestureBlocks(configWith(start), "w", "GESTURE_LONG_PRESS", [wait(2)]));
  assert.deepEqual(gestureBlocks(next, "GESTURE_SHORT_PRESS"), [wait(1)]);
  assert.deepEqual(gestureBlocks(next, "GESTURE_LONG_PRESS"), [wait(2)]);
  assert.equal(next.flows?.length, 3);
  assert.deepEqual(slotCounts(next), { tap: 1, double: 0, long: 1, hold: 0 });
});

test("a hold and a tap are exclusive", () => {
  const tapped: WireWidget = { id: "w", flows: [{ trigger: { gesture: "GESTURE_SHORT_PRESS" }, blocks: [wait(1)] }] };
  const held = widgetOf(setGestureBlocks(configWith(tapped), "w", "GESTURE_HOLD_START", [wait(2)]));
  assert.equal(holdsOf(held), true);
  assert.deepEqual(gestureBlocks(held, "GESTURE_SHORT_PRESS"), []);
  const tappedAgain = widgetOf(setGestureBlocks(configWith(held), "w", "GESTURE_SHORT_PRESS", [wait(3)]));
  assert.equal(holdsOf(tappedAgain), false);
});

test("clearing the last flow stores none", () => {
  const one: WireWidget = { id: "w", flows: [{ trigger: { gesture: "GESTURE_SHORT_PRESS" }, blocks: [wait(1)] }] };
  const cleared = widgetOf(setGestureBlocks(configWith(one), "w", "GESTURE_SHORT_PRESS", []));
  assert.equal("flows" in cleared, false);
});
