import assert from "node:assert/strict";
import { test } from "node:test";

import { ICONS, iconByName, searchIcons } from "../src/common/icons/icons.lib.ts";
import {
  duplicateWidget,
  setWidgetColors,
  setWidgetIcon,
} from "../src/modules/config/lib/editing.lib.ts";
import type { WireConfig } from "../src/services/api/api.ts";

/**
 * Icons and the widget properties around them.
 *
 * The icon set is DATA, so the tests are about its integrity — a duplicate name
 * or a path that is not a path would only show up as a wrong picture. The editing
 * rules follow the same "refused edit returns the same object" contract as the
 * rest of `editing.lib.ts`.
 */

function config(): WireConfig {
  return {
    schema: 1,
    rev: 3,
    profiles: [
      {
        id: "tablet",
        grid: { cols: 4, rows: 3 },
        pages: [
          {
            id: "page-1",
            name: "Main",
            widgets: [
              { id: "widget-1", col: 0, row: 0, w: 1, h: 1, props: { label: "Mute", icon: "mute" } },
            ],
          },
        ],
      },
    ],
  };
}

function firstWidget(document: WireConfig) {
  return document.profiles?.[0]?.pages?.[0]?.widgets?.[0];
}

// -- the icon set ------------------------------------------------------------

test("every icon has a unique name and at least one path", () => {
  const names = ICONS.map((icon) => icon.name);
  assert.equal(new Set(names).size, names.length, "icon names must be unique");

  for (const icon of ICONS) {
    assert.ok(icon.title.length > 0, `${icon.name} has no title`);
    assert.ok(icon.paths.length > 0, `${icon.name} has no paths`);
    for (const d of icon.paths) {
      // A path that does not begin a move command renders as nothing at all,
      // which looks exactly like an empty icon set.
      assert.match(d, /^M/, `${icon.name} has a path that does not start with M: ${d}`);
    }
  }
});

test("iconByName finds an icon, and answers null for one that is not there", () => {
  assert.equal(iconByName("play")?.title, "Play");
  assert.equal(iconByName("not-an-icon"), null);
});

test("searchIcons matches on name and on title, and returns all for no query", () => {
  assert.equal(searchIcons("").length, ICONS.length);
  assert.ok(searchIcons("mute").some((icon) => icon.name === "mute"));
  // "Microphone" is a title, not a name.
  assert.ok(searchIcons("microphone").some((icon) => icon.name === "mic"));
  assert.deepEqual(searchIcons("zzzz"), []);
});

// -- setWidgetIcon -----------------------------------------------------------

test("setWidgetIcon sets an icon and keeps the other props", () => {
  const widget = firstWidget(setWidgetIcon(config(), "widget-1", "play"));
  assert.equal(widget?.props?.icon, "play");
  assert.equal(widget?.props?.label, "Mute", "the label must survive");
});

test("clearing an icon removes the key, matching proto3's default omission", () => {
  const widget = firstWidget(setWidgetIcon(config(), "widget-1", null));
  assert.ok(widget?.props);
  assert.equal("icon" in widget.props, false);
  assert.equal(widget.props.label, "Mute");
});

test("an all-whitespace icon is treated as no icon", () => {
  const widget = firstWidget(setWidgetIcon(config(), "widget-1", "   "));
  assert.equal("icon" in (widget?.props ?? {}), false);
});

test("clearing the LAST prop drops `props` entirely, as the agent would", () => {
  const only: WireConfig = {
    schema: 1,
    rev: 1,
    profiles: [
      {
        id: "tablet",
        grid: { cols: 2, rows: 2 },
        pages: [{ id: "page-1", widgets: [{ id: "widget-1", props: { icon: "play" } }] }],
      },
    ],
  };
  const widget = firstWidget(setWidgetIcon(only, "widget-1", null));
  assert.ok(widget);
  assert.equal("props" in widget, false);
});

test("setWidgetIcon refuses an unknown widget by returning the same object", () => {
  const before = config();
  assert.equal(setWidgetIcon(before, "widget-nope", "play"), before);
});

// -- setWidgetColors ---------------------------------------------------------

test("setWidgetColors sets both colours, then clears them one at a time", () => {
  const coloured = setWidgetColors(config(), "widget-1", { bgColor: "#111111", fgColor: "#fafaf0" });
  assert.equal(firstWidget(coloured)?.props?.bgColor, "#111111");
  assert.equal(firstWidget(coloured)?.props?.fgColor, "#fafaf0");

  const cleared = setWidgetColors(coloured, "widget-1", { bgColor: "" });
  assert.equal("bgColor" in (firstWidget(cleared)?.props ?? {}), false);
  assert.equal(firstWidget(cleared)?.props?.fgColor, "#fafaf0", "the other colour must survive");
});

test("setWidgetColors refuses an unknown widget by returning the same object", () => {
  const before = config();
  assert.equal(setWidgetColors(before, "widget-nope", { bgColor: "#000000" }), before);
});

// -- duplicateWidget ---------------------------------------------------------

test("duplicateWidget copies the widget into the first free cell with a new id", () => {
  const next = duplicateWidget(config(), "widget-1");
  const widgets = next.profiles?.[0]?.pages?.[0]?.widgets ?? [];

  assert.equal(widgets.length, 2);
  const copy = widgets[1];
  assert.ok(copy);
  assert.notEqual(copy.id, "widget-1");
  assert.equal(copy.props?.icon, "mute", "the copy carries the props");
  // widget-1 sits at 0,0, so the copy takes the next cell along.
  assert.equal(copy.col, 1);
  assert.equal(copy.row, 0);
});

test("duplicateWidget refuses when the page has no room", () => {
  const full: WireConfig = {
    schema: 1,
    rev: 1,
    profiles: [
      {
        id: "tablet",
        grid: { cols: 1, rows: 1 },
        pages: [{ id: "page-1", widgets: [{ id: "widget-1", col: 0, row: 0, w: 1, h: 1 }] }],
      },
    ],
  };
  assert.equal(duplicateWidget(full, "widget-1"), full);
});

test("duplicateWidget refuses an unknown widget", () => {
  const before = config();
  assert.equal(duplicateWidget(before, "widget-nope"), before);
});
