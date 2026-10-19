import assert from "node:assert/strict";
import { test } from "node:test";

import { current, isConfirmed, needsReread, overlayPreviews, readingOf, rereadsOnOpen } from "../src/modules/config/lib/resource.lib.ts";

const isNull = (value: unknown) => value === null;
const idle = { value: undefined, error: null, busy: false };

test("no desktop is idle, whatever the store holds", () => {
  assert.equal(readingOf(null, { value: { rev: 1 }, error: null, busy: false }, isNull), "idle");
});

test("a desktop read for the first time is loading", () => {
  assert.equal(readingOf("fp", { ...idle, busy: true }, isNull), "loading");
  assert.equal(readingOf("fp", idle, isNull), "loading");
});

test("a failed read is failed even with an older document still held", () => {
  assert.equal(readingOf("fp", { value: { rev: 1 }, error: new Error("x"), busy: false }, isNull), "failed");
});

test("trying a failed read again shows it loading, not failed", () => {
  assert.equal(readingOf("fp", { value: { rev: 1 }, error: new Error("x"), busy: true }, isNull), "loading");
});

test("a desktop with no document yet is empty", () => {
  assert.equal(readingOf("fp", { value: null, error: null, busy: false }, isNull), "empty");
});

test("a re-read of the document on screen keeps it loaded", () => {
  assert.equal(readingOf("fp", { value: { rev: 1 }, error: null, busy: true }, isNull), "loaded");
});

test("the revision on screen is not read again", () => {
  assert.equal(needsReread(4, 4), false);
  assert.equal(needsReread(4, 5), true);
  assert.equal(needsReread(undefined, 0), true);
});

test("a push laid over the answer wins, and only for keys on this page", () => {
  const loaded = { a: { state: { label: "old" } }, b: { error: "bad template" } };
  const pushed = { a: { label: "new" }, b: { label: "lit" }, c: { label: "elsewhere" } };
  assert.deepEqual(overlayPreviews(loaded, pushed, ["a", "b"]), {
    a: { state: { label: "new" } },
    b: { error: "bad template", state: { label: "lit" } },
  });
});

test("a push that lands before the answer still shows", () => {
  assert.deepEqual(overlayPreviews(undefined, { a: { label: "early" } }, ["a"]), { a: { state: { label: "early" } } });
});

function snapshots(busy: boolean, error: Error | null) {
  return { snapshot: () => ({ busy, error }) };
}

test("a failed read that is still the latest is a failure", async () => {
  const cause = new Error("refused");
  await assert.rejects(current(snapshots(false, cause), ["fp"], Promise.reject(cause)), cause);
});

test("a failed read replaced by one still running is not a failure", async () => {
  await current(snapshots(true, null), ["fp"], Promise.reject(new Error("refused")));
});

test("a failed read replaced by one that worked is not a failure", async () => {
  await current(snapshots(false, null), ["fp"], Promise.reject(new Error("refused")));
});

test("a failed read replaced by one that failed leaves the newer failure to its own caller", async () => {
  await current(snapshots(false, new Error("newer")), ["fp"], Promise.reject(new Error("older")));
});

test("the first open leaves reads already running alone, and a later one re-reads everything", () => {
  const ids = [["a", "config"], ["a", "facts"], ["a", "catalogue"], ["b", "config"], ["a", "previews", "[]"]];
  const busy = (id: readonly unknown[]) => id[1] === "config";
  assert.deepEqual(rereadsOnOpen(ids, "a", true, busy), [["a", "facts"], ["a", "previews", "[]"]]);
  assert.deepEqual(rereadsOnOpen(ids, "a", false, busy), [["a", "config"], ["a", "facts"], ["a", "previews", "[]"]]);
});

test("an unconfirmed write is confirmed once the watched revision moves on", () => {
  const pending = { desktopId: "a", watch: "deck" as const, rev: 4, until: 0 };
  assert.equal(isConfirmed(pending, 4), false);
  assert.equal(isConfirmed(pending, undefined), false);
  assert.equal(isConfirmed(pending, 5), true);
  assert.equal(isConfirmed({ ...pending, rev: undefined }, 1), true);
  assert.equal(isConfirmed({ ...pending, watch: null }, 5), false);
});
