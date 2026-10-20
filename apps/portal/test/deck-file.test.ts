import assert from "node:assert/strict";
import { test } from "node:test";

import type { ActionDefinition, WireConfig, WireWidget } from "../src/services/api/api.ts";
import { appendDeck, checkDeck, deckConcerns, exportDeck, pageCount, parseDeck } from "../src/modules/config/lib/deck.file.lib.ts";

const config: WireConfig = {
  schema: 4,
  rev: 7,
  profiles: [{ id: "p", pages: [{ id: "a", name: "Home", widgets: [] }, { id: "b", name: "Stream", widgets: [] }] }],
};

test("export is minified and leaves the revision out", () => {
  const text = exportDeck(config);
  assert.ok(!text.includes("\n") && !text.includes(": "));
  assert.equal("rev" in JSON.parse(text), false);
  assert.deepEqual(config.rev, 7);
});

test("import round-trips onto the current revision", () => {
  const imported = parseDeck(exportDeck(config), { schema: 4, rev: 12, profiles: [] });
  assert.equal(imported.rev, 12);
  assert.deepEqual(imported.profiles, config.profiles);
  assert.equal(pageCount(imported), 2);
});

const grid = { cols: 4, rows: 2 };
const go = (page: string) => ({ trigger: { gesture: "GESTURE_SHORT_PRESS" as const }, blocks: [{ block: "doAction" as const, plugin: "deck", action: "go_to_page", args: { page: { text: page } } }] });
const obs = { trigger: { gesture: "GESTURE_SHORT_PRESS" as const }, blocks: [{ block: "doAction" as const, plugin: "obs", action: "record", args: {} }] };
const catalogue = [{ plugin: "obs", action: "record" }] as unknown as ActionDefinition[];
const here: WireConfig = { schema: 4, rev: 2, profiles: [{ id: "p", grid, pages: [{ id: "page-1", name: "Home", widgets: [{ id: "widget-1", col: 0, row: 0, w: 1, h: 1 }] }] }] };
const file = (widgets: WireWidget[], extra: Partial<WireConfig> = {}): WireConfig => ({
  schema: 4,
  profiles: [{ id: "p", grid, pages: [{ id: "page-1", name: "Stream", widgets }, { id: "page-2", name: "Chat", widgets: [] }] }],
  ...extra,
});

test("a plugin this desktop lacks is named", () => {
  const check = checkDeck(file([{ id: "widget-1", col: 0, row: 0, w: 1, h: 1, flows: [obs] }]), here, "replace", "p", []);
  assert.deepEqual(check.missing, ["obs"]);
  assert.deepEqual(checkDeck(file([{ id: "widget-1", col: 0, row: 0, w: 1, h: 1, flows: [obs] }]), here, "replace", "p", catalogue).missing, []);
});

test("keys that would break are listed", () => {
  const check = checkDeck(
    file([
      { id: "widget-1", col: 0, row: 0, w: 2, h: 1 },
      { id: "widget-2", col: 1, row: 0, w: 1, h: 1 },
      { id: "widget-3", col: 9, row: 0, w: 1, h: 1 },
      { id: "widget-4", col: 0, row: 1, w: 1, h: 1, flows: [go("Nowhere")] },
      { id: "widget-4", col: 1, row: 1, w: 1, h: 1 },
    ]),
    here,
    "replace",
    "p",
    catalogue,
  );
  assert.deepEqual(check.problems, ["Two keys share the id “widget-4”."]);
  const placed = checkDeck(
    file([
      { id: "widget-1", col: 0, row: 0, w: 2, h: 1 },
      { id: "widget-2", col: 1, row: 0, w: 1, h: 1 },
      { id: "widget-3", col: 9, row: 0, w: 1, h: 1 },
      { id: "widget-4", col: 0, row: 1, w: 1, h: 1, flows: [go("Nowhere"), { blocks: [{ block: "doAction", plugin: "obs", action: "stream", args: {} }] }] },
      { id: "widget-5", col: 1, row: 1, w: 1, h: 1, flows: [go("Chat")] },
    ]),
    here,
    "replace",
    "p",
    catalogue,
  );
  assert.equal(placed.problems.length, 4);
  assert.match(placed.problems[0]!, /widget-2.*another widget/);
  assert.match(placed.problems[1]!, /widget-3.*outside the grid/);
  assert.match(placed.problems[2]!, /“Nowhere”/);
  assert.match(placed.problems[3]!, /no “stream” action/);
});

test("append checks against this desktop's grid", () => {
  const wide = file([{ id: "widget-1", col: 3, row: 0, w: 1, h: 1 }], { profiles: [{ id: "q", grid: { cols: 8, rows: 2 }, pages: [{ id: "page-1", widgets: [{ id: "widget-1", col: 6, row: 0, w: 1, h: 1 }] }] }] });
  assert.equal(checkDeck(wide, here, "replace", "p", []).problems.length, 0);
  assert.match(checkDeck(wide, here, "append", "p", []).problems[0]!, /outside the grid/);
});

test("append adds pages with fresh ids and follows page links", () => {
  const next = appendDeck(here, "p", file([{ id: "widget-1", col: 0, row: 0, w: 1, h: 1, flows: [go("page-2")] }]));
  const pages = next.profiles![0]!.pages!;
  assert.deepEqual(pages.map((page) => page.id), ["page-1", "page-2", "page-3"]);
  assert.equal(pages[1]!.widgets![0]!.id === "widget-1", false);
  const block = pages[1]!.widgets![0]!.flows![0]!.blocks![0] as unknown as { args: { page: { text: string } } };
  assert.equal(block.args.page.text, "page-3");
  assert.equal(next.rev, 2);
});

test("import refuses what is not a deck", () => {
  const current = { schema: 4, rev: 1, profiles: [] };
  assert.throws(() => parseDeck("not json", current), /not JSON/);
  assert.throws(() => parseDeck("[]", current), /not a FlexDeck deck/);
  assert.throws(() => parseDeck(`{"schema":4}`, current), /no profiles/);
  assert.throws(() => parseDeck(`{"profiles":[]}`, current), /no schema/);
  assert.throws(() => parseDeck(`{"schema":9,"profiles":[]}`, current), /newer FlexDeck/);
});

test("programs, keystrokes and outside addresses are flagged before import, worst first", () => {
  const run = { trigger: { gesture: "GESTURE_SHORT_PRESS" as const }, blocks: [{ block: "doAction" as const, plugin: "launcher", action: "run", args: { program: { text: "powershell" } } }] };
  const typed = { trigger: { event: { plugin: "timer", event: "finished" } }, blocks: [{ block: "doAction" as const, plugin: "core", action: "text", args: { text: { text: "hi" } } }] };
  const concerns = deckConcerns(
    file([
      { id: "widget-1", col: 0, row: 0, w: 1, h: 1, flows: [obs] },
      { id: "widget-2", col: 1, row: 0, w: 1, h: 1, props: { label: "Mute" }, flows: [run, typed] },
      { id: "widget-3", col: 2, row: 0, w: 1, h: 1, sources: [{ source: "ha/state", args: { url: "https://evil.example/x" } }] },
    ] as WireWidget[]),
  );
  assert.deepEqual(
    concerns.map((concern) => [concern.where, concern.what, concern.severe]),
    [
      ["Stream › Mute", "runs a program: powershell", true],
      ["Stream › Mute", "types text: hi", true],
      ["Stream › widget-3", "points ha/state at evil.example", true],
      ["Stream › widget-1", "uses the obs plugin (record)", false],
    ],
  );
  assert.deepEqual(deckConcerns(config), []);
});
