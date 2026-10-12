import assert from "node:assert/strict";
import { test } from "node:test";

import type { GridSpec } from "@flexdeck/grid";

import type { ActionDefinition, WireConfig, WireWidget } from "../src/services/api/api.ts";
import { gridSpecOf, profileOf } from "../src/modules/config/lib/config.lib.ts";
import {
  addWidget,
  findWidget,
  lightActiveKeys,
  relightActive,
  firstFreeCell,
  limitsFor,
  moveWidget,
  nextWidgetId,
  placementProblem,
  pressActionOf,
  removeWidget,
  resizeWidget,
  setActiveLight,
  setWidgetAction,
  setWidgetBinding,
  setWidgetLabel,
  spansOf,
} from "../src/modules/config/lib/editing.lib.ts";

function configWith(widgets: WireWidget[]): WireConfig {
  return {
    schema: 2,
    rev: 1,
    profiles: [
      {
        id: "tab1",
        grid: { cols: 4, rows: 3, gapX: 0, gapY: 0, usableW: 400, usableH: 300 },
        pages: [{ id: "home", widgets }],
      },
    ],
  };
}

function button(id: string, col: number, row: number, extra: Partial<WireWidget> = {}): WireWidget {
  return { id, type: "WIDGET_TYPE_BUTTON", col, row, w: 1, h: 1, ...extra };
}

function spec(config: WireConfig): GridSpec {
  return gridSpecOf(profileOf(config));
}

function positionOf(config: WireConfig, id: string): { col: number; row: number } {
  const found = findWidget(config, id);
  assert.ok(found, `${id} should still exist`);
  return { col: found.widget.col ?? 0, row: found.widget.row ?? 0 };
}

test("a move onto an occupied cell is refused", () => {
  // Refused means the configuration comes back exactly as it was, so a caller
  // cannot half-apply an edit.
  const config = configWith([button("a", 0, 0), button("b", 1, 0)]);

  const after = moveWidget(config, "a", 1, 0);

  assert.deepEqual(positionOf(after, "a"), { col: 0, row: 0 });
  assert.deepEqual(positionOf(after, "b"), { col: 1, row: 0 }, "and the other one is untouched");
});

test("a move off the edge of the grid is refused", () => {
  const config = configWith([button("a", 3, 2)]);

  // Both directions: past the last column and past the last row.
  assert.deepEqual(positionOf(moveWidget(config, "a", 4, 0), "a"), { col: 3, row: 2 });
  assert.deepEqual(positionOf(moveWidget(config, "a", 0, 3), "a"), { col: 3, row: 2 });
  assert.deepEqual(positionOf(moveWidget(config, "a", -1, 0), "a"), { col: 3, row: 2 });
});

test("a move to where the widget already is succeeds", () => {
  // A widget is not allowed to overlap *itself*: without that, nudging a widget
  // into its own cell would be refused and the widget would appear stuck.
  const config = configWith([button("a", 1, 1)]);

  assert.deepEqual(moveWidget(config, "a", 1, 1), config);
});

test("a move nearby is allowed", () => {
  const config = configWith([button("a", 0, 0)]);

  assert.deepEqual(positionOf(moveWidget(config, "a", 2, 1), "a"), { col: 2, row: 1 });
});

test("moving a widget that is not there changes nothing", () => {
  // A stale click — a widget deleted in another tab — must be a no-op, not a
  // crash and not a spurious save.
  const config = configWith([button("a", 0, 0)]);

  assert.deepEqual(moveWidget(config, "ghost", 2, 2), config);
});

test("a span that would hang off the edge is refused", () => {
  const config = configWith([button("a", 0, 0, { w: 2, h: 1 })]);

  // Two wide at column 3 needs columns 3 and 4; there are only four.
  const after = resizeWidget(config, "a", 2, 1);
  assert.deepEqual(after, config, "unchanged");

  const moved = moveWidget(config, "a", 3, 0);
  assert.deepEqual(positionOf(moved, "a"), { col: 0, row: 0 });
});

test("a resize respects the type's limits", () => {
  const config = configWith([button("a", 0, 0)]);

  // Buttons are allowed up to 4x4, but this grid is only 4x3 — so 4x3 is the
  // largest span that both the limit and the grid permit.
  assert.equal(findWidget(resizeWidget(config, "a", 4, 3), "a")?.widget.w, 4);
  // Wider than the type allows is refused, even though it would nearly fit.
  assert.equal(findWidget(resizeWidget(config, "a", 6, 1), "a")?.widget.w, 1);
  // ...and not below one cell.
  assert.equal(findWidget(resizeWidget(config, "a", 0, 1), "a")?.widget.w, 1);
});

test("an unknown widget type gets permissive limits rather than being refused", () => {
  // A newer portal may know types this build does not; refusing to resize them
  // would make an edited configuration un-editable.
  assert.equal(limitsFor("WIDGET_TYPE_FROM_THE_FUTURE").maxW, 8);

  const config = configWith([
    { id: "a", type: "WIDGET_TYPE_FROM_THE_FUTURE", col: 0, row: 0, w: 1, h: 1 },
  ]);

  assert.equal(
    findWidget(resizeWidget(config, "a", 4, 3), "a")?.widget.w,
    4,
    "a button would have been refused at 4 wide only if it exceeded 4 — this proves the limit is permissive",
  );
});

test("a widget is removed by omission", () => {
  // PUT replaces the whole document, so deleting means the widget is simply not
  // in what gets sent.
  const config = configWith([button("a", 0, 0), button("b", 1, 0)]);

  const after = removeWidget(config, "a");

  assert.equal(findWidget(after, "a"), null);
  assert.ok(findWidget(after, "b"), "the other one survives");
});

test("a new widget lands in the first free cell", () => {
  const config = configWith([button("a", 0, 0), button("b", 1, 0)]);

  const after = addWidget(config, { type: "WIDGET_TYPE_BUTTON" });

  const added = after.profiles?.[0]?.pages?.[0]?.widgets?.at(-1);
  assert.equal(added?.id, "widget-1");
  assert.deepEqual({ col: added?.col, row: added?.row }, { col: 2, row: 0 });
});

test("a new widget can wrap to the next row", () => {
  // Every cell of the first row is taken, so the first free cell is the start of
  // the second.
  const fullRow = [button("a", 0, 0), button("b", 1, 0), button("c", 2, 0), button("d", 3, 0)];

  const after = addWidget(configWith(fullRow), { type: "WIDGET_TYPE_BUTTON" });
  const added = after.profiles?.[0]?.pages?.[0]?.widgets?.at(-1);

  assert.deepEqual({ col: added?.col, row: added?.row }, { col: 0, row: 1 });
});

test("adding to a full grid is refused rather than placing a widget on top of another", () => {
  const spec: GridSpec = { cols: 1, rows: 1, gapX: 0, gapY: 0, usableW: 100, usableH: 100 };
  assert.equal(firstFreeCell(spec, [{ id: "a", col: 0, row: 0, w: 1, h: 1 }]), null);

  // A grid of exactly one cell, already occupied.
  const full: WireConfig = {
    schema: 2,
    rev: 1,
    profiles: [
      {
        id: "tab1",
        grid: { cols: 1, rows: 1, gapX: 0, gapY: 0, usableW: 100, usableH: 100 },
        pages: [{ id: "home", widgets: [button("a", 0, 0)] }],
      },
    ],
  };

  const after = addWidget(full, { type: "WIDGET_TYPE_BUTTON" });

  assert.equal(after.profiles?.[0]?.pages?.[0]?.widgets?.length, 1, "nothing was added");
});

test("a new widget id does not collide with one already in use", () => {
  assert.equal(nextWidgetId([]), "widget-1");
  assert.equal(nextWidgetId(["widget-1"]), "widget-2");
  assert.equal(nextWidgetId(["widget-2"]), "widget-1", "gaps are reused");
  assert.equal(nextWidgetId(["widget-1", "widget-2", "widget-3"]), "widget-4");
});

test("a widget added to a new page gets an id no other page uses", () => {
  const base = configWith([button("widget-1", 0, 0)]);
  const config: WireConfig = {
    ...base,
    profiles: [{ ...base.profiles![0]!, pages: [...base.profiles![0]!.pages!, { id: "media", widgets: [] }] }],
  };

  const after = addWidget(config, { type: "WIDGET_TYPE_BUTTON" }, "media");

  assert.equal(after.profiles?.[0]?.pages?.[1]?.widgets?.[0]?.id, "widget-2");
});

test("a label is trimmed before it is stored", () => {
  const config = configWith([button("a", 0, 0)]);

  assert.equal(findWidget(setWidgetLabel(config, "a", "  Mute  "), "a")?.widget.props?.label, "Mute");
});

test("clearing a label removes it rather than storing an empty string", () => {
  // A present-but-empty string would differ from what the agent sends back, so
  // every save would look like a change.
  const config = configWith([button("a", 0, 0, { props: { label: "Mute" } })]);

  assert.equal(findWidget(setWidgetLabel(config, "a", "   "), "a")?.widget.props?.label, undefined);
});

test("emptying the last property drops the props object entirely", () => {
  // Same reason: proto3 JSON omits empty messages.
  const config = configWith([button("a", 0, 0, { props: { label: "Mute" } })]);

  assert.equal(findWidget(setWidgetLabel(config, "a", ""), "a")?.widget.props, undefined);
});

test("other properties survive a label edit", () => {
  const config = configWith([button("a", 0, 0, { props: { label: "Mute", icon: "mic-off" } })]);

  assert.equal(findWidget(setWidgetLabel(config, "a", "Silence"), "a")?.widget.props?.icon, "mic-off");
});

test("placement problems are described in words", () => {
  const spec: GridSpec = { cols: 2, rows: 2, gapX: 0, gapY: 0, usableW: 100, usableH: 100 };
  const occupied = [{ id: "a", col: 0, row: 0, w: 1, h: 1 }];

  assert.equal(placementProblem(spec, occupied, { id: "b", col: 0, row: 0, w: 1, h: 1 }), "overlap");
  assert.equal(placementProblem(spec, occupied, { id: "b", col: 2, row: 0, w: 1, h: 1 }), "out_of_bounds");
  assert.equal(placementProblem(spec, occupied, { id: "b", col: 1, row: 0, w: 1, h: 1 }), null);
});

test("spans default to one cell when the agent omitted them", () => {
  assert.deepEqual(spansOf({ id: "home", widgets: [{ id: "a" }] }), [
    { id: "a", col: 0, row: 0, w: 1, h: 1 },
  ]);
});

test("the grid spec used for validation matches the one used to render", () => {
  // Validation and rendering reading different numbers would let a move be
  // accepted and then render outside the frame.
  const config = configWith([button("a", 0, 0)]);
  assert.deepEqual(spec(config), gridSpecOf(profileOf(config)));
});

test("an action can be attached to a widget", () => {
  // Without this the editor could lay out buttons that do nothing at all.
  const config = configWith([button("a", 0, 0)]);

  const after = setWidgetAction(config, "a", {
    plugin: "core",
    action: "hotkey",
    args: { keys: "ctrl+shift+m" },
  });

  const found = findWidget(after, "a");
  assert.deepEqual(pressActionOf(found!.widget), {
    plugin: "core",
    action: "hotkey",
    args: { keys: "ctrl+shift+m" },
  });
});

test("setting an action replaces whatever was there before", () => {
  // Appending instead would leave the user with a widget that fires something
  // they can neither see nor remove.
  const config = setWidgetAction(configWith([button("a", 0, 0)]), "a", {
    plugin: "core",
    action: "text",
    args: { text: "first" },
  });

  const after = setWidgetAction(config, "a", {
    plugin: "core",
    action: "hotkey",
    args: { keys: "ctrl+m" },
  });

  const found = findWidget(after, "a");
  assert.equal(found?.widget.flows?.length, 1, "one short-press flow");
  assert.equal(found?.widget.flows?.[0]?.blocks?.length, 1);
  assert.equal(pressActionOf(found!.widget)?.action, "hotkey");
});

test("an action is stored as a short-press flow of one step", () => {
  const after = setWidgetAction(configWith([button("a", 0, 0)]), "a", {
    plugin: "obs",
    action: "set_scene",
    args: { scene: "Live", note: "{{ obs.scene }}" },
  });
  assert.deepEqual(findWidget(after, "a")?.widget.flows, [
    {
      trigger: { gesture: "GESTURE_SHORT_PRESS" },
      blocks: [
        {
          block: "doAction",
          plugin: "obs",
          action: "set_scene",
          args: { scene: { text: "Live" }, note: { variable: "obs.scene" } },
        },
      ],
    },
  ]);
  assert.deepEqual(pressActionOf(findWidget(after, "a")!.widget)?.args, { scene: "Live", note: "{{obs.scene}}" });
});

test("setting the press action keeps the widget's other flows", () => {
  const longPress = { trigger: { gesture: "GESTURE_LONG_PRESS" as const }, blocks: [] };
  const config = configWith([button("a", 0, 0, { flows: [longPress] })]);
  const after = setWidgetAction(config, "a", { plugin: "core", action: "hotkey", args: { keys: "m" } });
  assert.equal(findWidget(after, "a")?.widget.flows?.length, 2);
  const cleared = setWidgetAction(after, "a", null);
  assert.deepEqual(findWidget(cleared, "a")?.widget.flows, [longPress]);
});

test("clearing an action drops the empty flows list", () => {
  // proto3 JSON omits empty lists, so a stored `flows: []` would differ
  // from what the agent sends back and every save would look like a change.
  const config = setWidgetAction(configWith([button("a", 0, 0)]), "a", {
    plugin: "core",
    action: "hotkey",
    args: { keys: "ctrl+m" },
  });

  const after = setWidgetAction(config, "a", null);

  assert.equal(findWidget(after, "a")?.widget.flows, undefined);
  assert.equal(pressActionOf(findWidget(after, "a")!.widget), null);
});

test("an action with no arguments survives the round trip", () => {
  const config = configWith([button("a", 0, 0)]);

  const after = setWidgetAction(config, "a", { plugin: "core", action: "nothing" });

  assert.equal(pressActionOf(findWidget(after, "a")!.widget)?.action, "nothing");
});

test("editing an action leaves the rest of the widget alone", () => {
  const config = configWith([button("a", 0, 0, { props: { label: "Mute" }, w: 2 })]);

  const after = setWidgetAction(config, "a", { plugin: "core", action: "hotkey", args: { keys: "m" } });

  const found = findWidget(after, "a");
  assert.equal(found?.widget.props?.label, "Mute");
  assert.equal(found?.widget.w, 2);
  assert.deepEqual({ col: found?.widget.col, row: found?.widget.row }, { col: 0, row: 0 });
});

test("setting an action on a widget that is not there changes nothing", () => {
  const config = configWith([button("a", 0, 0)]);

  assert.deepEqual(setWidgetAction(config, "ghost", { plugin: "core", action: "hotkey" }), config);
});

const setScene: ActionDefinition = {
  plugin: "obs",
  action: "set_scene",
  description: "Switch the live scene",
  arguments: [{ name: "scene", description: "", example: "Scene" }],
  active: { variable: "obs.scene", equals: "scene" },
};

test("a scene key lights while its scene is live, and stops when it no longer switches scenes", () => {
  const config = configWith([button("a", 0, 0)]);
  const lit = setActiveLight(config, "a", setScene, { scene: "CS2" });
  const states = findWidget(lit, "a")?.widget.states;
  assert.deepEqual(states?.variable, { variable: "obs.scene", cases: [{ value: "CS2", state: "on" }], otherwise: "off" });
  assert.deepEqual(states?.looks?.map((look) => look.id), ["off", "on"]);

  const moved = setActiveLight(lit, "a", setScene, { scene: "PS5" });
  assert.equal(findWidget(moved, "a")?.widget.states?.variable?.cases?.[0]?.value, "PS5");

  const plain = { ...setScene, active: undefined };
  assert.equal(findWidget(setActiveLight(moved, "a", plain, { scene: "PS5" }), "a")?.widget.states, undefined);
  assert.equal(findWidget(setActiveLight(moved, "a", null, {}), "a")?.widget.states, undefined);
});

test("a key's own states are never replaced by the active light", () => {
  const bound = setWidgetBinding(configWith([button("a", 0, 0)]), "a", { stateVariable: "obs.recording", labelTemplate: "" });
  const after = setActiveLight(bound, "a", setScene, { scene: "CS2" });
  assert.deepEqual(findWidget(after, "a")?.widget.states, findWidget(bound, "a")?.widget.states);
  assert.equal(findWidget(setActiveLight(bound, "a", null, {}), "a")?.widget.states?.variable?.variable, "obs.recording");
});

test("scene keys made before the active light get it; others are left as they are", () => {
  const scene = button("a", 0, 0);
  const other = button("b", 1, 0);
  const config = setWidgetAction(setWidgetAction(configWith([scene, other]), "a", { plugin: "obs", action: "set_scene", args: { scene: "CS2" } }), "b", { plugin: "obs", action: "start_recording", args: {} });
  const lit = lightActiveKeys(config, [setScene]);
  assert.equal(findWidget(lit, "a")?.widget.states?.variable?.cases?.[0]?.value, "CS2");
  assert.equal(findWidget(lit, "b")?.widget.states, undefined);
  assert.equal(lightActiveKeys(lit, [setScene]), lit, "nothing left to light is no change");
});

test("changing a scene key's tap by hand moves its light with it", () => {
  const config = setWidgetAction(configWith([button("a", 0, 0)]), "a", { plugin: "obs", action: "set_scene", args: { scene: "CS2" } });
  const lit = relightActive(config, "a", [setScene]);
  assert.equal(findWidget(lit, "a")?.widget.states?.variable?.cases?.[0]?.value, "CS2");
  const moved = relightActive(setWidgetAction(lit, "a", { plugin: "obs", action: "set_scene", args: { scene: "PS5" } }), "a", [setScene]);
  assert.equal(findWidget(moved, "a")?.widget.states?.variable?.cases?.[0]?.value, "PS5");
  // Not yet named: no light, and no "on" for an empty scene.
  const unnamed = relightActive(setWidgetAction(moved, "a", { plugin: "obs", action: "set_scene", args: {} }), "a", [setScene]);
  assert.equal(findWidget(unnamed, "a")?.widget.states, undefined);
});
