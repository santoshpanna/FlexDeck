import assert from "node:assert/strict";
import { test } from "node:test";

import type { WireConfig } from "../src/services/api/api.ts";
import {
  pageById,
  pageIndexOf,
  pageNameOf,
  pagesOf,
  profileOf,
} from "../src/modules/config/lib/config.lib.ts";
import {
  addPage,
  duplicatePage,
  findPage,
  movePage,
  nextPageId,
  removePage,
  renamePage,
  withPage,
} from "../src/modules/config/lib/editing.lib.ts";

/**
 * Pages.
 *
 * The interesting rules are all about what a page operation REFUSES: the last
 * page cannot be deleted, a page cannot move off either end, and an untouched
 * rename must not spend a revision. Each refusal is the same object handed back,
 * which is how the UI tells "declined" apart from "did nothing".
 */

function config(): WireConfig {
  return {
    schema: 2,
    rev: 4,
    profiles: [
      {
        id: "tablet",
        name: "Tablet",
        grid: { cols: 4, rows: 3, gapX: 8, gapY: 8, usableW: 800, usableH: 600 },
        pages: [
          { id: "page-1", name: "Main", widgets: [{ id: "widget-1", col: 0, row: 0 }] },
          { id: "page-2", name: "Media", widgets: [{ id: "widget-2", col: 1, row: 0 }] },
        ],
      },
    ],
  };
}

function widgetIds(document: WireConfig): string[] {
  return (document.profiles ?? []).flatMap((profile) =>
    (profile.pages ?? []).flatMap((page) => (page.widgets ?? []).map((widget) => widget.id)),
  );
}

// -- reading -----------------------------------------------------------------

test("pagesOf returns the pages in order, and nothing without a profile", () => {
  assert.deepEqual(
    pagesOf(profileOf(config())).map((page) => page.id),
    ["page-1", "page-2"],
  );
  assert.deepEqual(pagesOf(null), []);
});

test("pageById finds a page, and answers null for an unknown or absent id", () => {
  const profile = profileOf(config());
  assert.equal(pageById(profile, "page-2")?.name, "Media");
  assert.equal(pageById(profile, "page-nope"), null);
  assert.equal(pageById(profile, null), null);
});

test("pageIndexOf reports the position, and -1 for a page that is not there", () => {
  const profile = profileOf(config());
  assert.equal(pageIndexOf(profile, "page-2"), 1);
  assert.equal(pageIndexOf(profile, "page-nope"), -1);
});

test("a page with no name (or only whitespace) is called by its id", () => {
  assert.equal(pageNameOf({ id: "page-9" }), "page-9");
  assert.equal(pageNameOf({ id: "page-9", name: "   " }), "page-9");
  assert.equal(pageNameOf({ id: "page-9", name: " Setup " }), "Setup");
});

test("nextPageId fills the first gap rather than always counting up", () => {
  assert.equal(nextPageId(["page-1", "page-2"]), "page-3");
  assert.equal(nextPageId(["page-1", "page-3"]), "page-2");
  assert.equal(nextPageId([]), "page-1");
});

// -- withPage ----------------------------------------------------------------

test("withPage applies an update in place and returns the same object when absent", () => {
  const before = config();
  const after = withPage(before, "page-2", (page) => ({ ...page, name: "Shows" }));
  assert.equal(after.profiles?.[0]?.pages?.[1]?.name, "Shows");
  assert.notEqual(after, before);

  // A stale id is a no-op, not a crash and not a new document.
  assert.equal(withPage(before, "page-nope", (page) => page), before);
});

// -- adding ------------------------------------------------------------------

test("addPage appends a page with the caller's id and optional name", () => {
  const next = addPage(config(), "page-3", "Games");
  assert.deepEqual(
    next.profiles?.[0]?.pages?.map((page) => page.id),
    ["page-1", "page-2", "page-3"],
  );
  assert.equal(next.profiles?.[0]?.pages?.[2]?.name, "Games");
});

test("addPage without a name leaves the key ABSENT, matching the wire", () => {
  const page = addPage(config(), "page-3").profiles?.[0]?.pages?.[2];
  assert.ok(page);
  // proto3 omits defaults, so a present-but-empty name would look like a change.
  assert.equal("name" in page, false);
});

test("addPage refuses, unchanged, when there is no profile to add to", () => {
  const empty: WireConfig = { schema: 2, rev: 1, profiles: [] };
  assert.equal(addPage(empty, "page-1"), empty);
});

// -- removing ----------------------------------------------------------------

test("removePage drops the page and leaves the others alone", () => {
  const next = removePage(config(), "page-1");
  assert.deepEqual(
    next.profiles?.[0]?.pages?.map((page) => page.id),
    ["page-2"],
  );
  assert.equal(next.profiles?.[0]?.pages?.[0]?.name, "Media");
});

test("removePage refuses to remove the LAST page", () => {
  const single: WireConfig = {
    schema: 2,
    rev: 2,
    profiles: [{ id: "tablet", pages: [{ id: "page-1" }] }],
  };
  assert.equal(removePage(single, "page-1"), single);
});

test("removePage refuses an unknown id rather than rebuilding the document", () => {
  const before = config();
  assert.equal(removePage(before, "page-nope"), before);
});

// -- renaming ----------------------------------------------------------------

test("renamePage trims the name it stores", () => {
  const next = renamePage(config(), "page-1", "  Cockpit  ");
  assert.equal(next.profiles?.[0]?.pages?.[0]?.name, "Cockpit");
});

test("renamePage to blank removes the name key, so a save is a no-op shape", () => {
  const page = renamePage(config(), "page-1", "   ").profiles?.[0]?.pages?.[0];
  assert.ok(page);
  assert.equal("name" in page, false);
});

test("renamePage returns the same object when the name already matches", () => {
  const before = config();
  assert.equal(renamePage(before, "page-1", "Main"), before);
  assert.equal(renamePage(before, "page-1", "  Main  "), before);
  assert.equal(renamePage(before, "page-nope", "x"), before);
});

// -- reordering --------------------------------------------------------------

test("movePage swaps a page with its neighbour", () => {
  const down = movePage(config(), "page-1", 1);
  assert.deepEqual(
    down.profiles?.[0]?.pages?.map((page) => page.id),
    ["page-2", "page-1"],
  );

  const up = movePage(down, "page-1", -1);
  assert.deepEqual(
    up.profiles?.[0]?.pages?.map((page) => page.id),
    ["page-1", "page-2"],
  );
});

test("movePage refuses at either end rather than wrapping", () => {
  const before = config();
  assert.equal(movePage(before, "page-1", -1), before);
  assert.equal(movePage(before, "page-2", 1), before);
  assert.equal(movePage(before, "page-nope", 1), before);
});

// -- duplicating -------------------------------------------------------------

test("duplicatePage inserts the copy directly after the original", () => {
  const next = duplicatePage(config(), "page-1", "page-3");
  assert.deepEqual(
    next.profiles?.[0]?.pages?.map((page) => page.id),
    ["page-1", "page-3", "page-2"],
  );
  assert.equal(next.profiles?.[0]?.pages?.[1]?.name, "Main copy");
});

test("duplicatePage remaps widget ids so none collide across the document", () => {
  const next = duplicatePage(config(), "page-1", "page-3");
  const ids = widgetIds(next);

  assert.equal(ids.length, 3);
  assert.equal(new Set(ids).size, ids.length, "every widget id must be unique");

  // The copy sits directly after the original, so these are its two pages.
  const original = next.profiles?.[0]?.pages?.[0]?.widgets?.[0]?.id;
  const copy = next.profiles?.[0]?.pages?.[1]?.widgets?.[0]?.id;
  assert.equal(original, "widget-1");
  assert.notEqual(copy, original, "the copy must not reuse the original's id");
});

test("duplicatePage omits widgets for an empty page, matching the wire", () => {
  const empty: WireConfig = {
    schema: 2,
    rev: 1,
    profiles: [{ id: "tablet", pages: [{ id: "page-1" }] }],
  };
  const copy = duplicatePage(empty, "page-1", "page-2").profiles?.[0]?.pages?.[1];
  assert.ok(copy);
  assert.equal("widgets" in copy, false);
});

test("duplicatePage refuses an unknown page", () => {
  const before = config();
  assert.equal(duplicatePage(before, "page-nope", "page-3"), before);
});

test("findPage finds a page anywhere in the document", () => {
  assert.equal(findPage(config(), "page-2")?.name, "Media");
  assert.equal(findPage(config(), "page-nope"), null);
  assert.equal(findPage(null, "page-1"), null);
});
