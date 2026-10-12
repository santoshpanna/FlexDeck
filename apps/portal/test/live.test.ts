import assert from "node:assert/strict";
import { test } from "node:test";

import { parseLive } from "../src/services/api/live.ts";

test("a revision arrives as a number, from a patch or a welcome", () => {
  // 64-bit numbers are strings in proto3 JSON.
  assert.deepEqual(parseLive(JSON.stringify({ patch: { rev: "42", patch: "[]" } })), { kind: "revision", rev: 42 });
  assert.deepEqual(parseLive(JSON.stringify({ welcome: { rev: "7", config: {} } })), { kind: "revision", rev: 7 });
});

test("a key's look arrives without the fields a preview does not draw", () => {
  const frame = { state: { widgetId: "cs2", ok: true, stateId: "on", label: "CS2", zonesOn: ["play"] } };
  assert.deepEqual(parseLive(JSON.stringify(frame)), {
    kind: "state",
    widgetId: "cs2",
    state: { stateId: "on", label: "CS2", zonesOn: ["play"] },
  });
});

test("pages, devices and refusals are told apart", () => {
  assert.deepEqual(parseLive(JSON.stringify({ page: { pageId: "stream" } })), { kind: "page", pageId: "stream", step: 0 });
  assert.deepEqual(parseLive(JSON.stringify({ devices: {} })), { kind: "devices" });
  assert.deepEqual(parseLive(JSON.stringify({ error: { code: "unauthorised", message: "revoked" } })), {
    code: "unauthorised",
    message: "revoked",
  });
});

test("anything unknown or unreadable is ignored, so a newer desktop never breaks an older page", () => {
  assert.equal(parseLive(JSON.stringify({ series: { variable: "x" } })), null);
  assert.equal(parseLive("not json"), null);
});

test("a source's run arrives with its outcome", () => {
  // int64 travels as a string; a run that never worked has no error text.
  assert.deepEqual(parseLive(JSON.stringify({ source: { source: "src.weather", ok: true, updated: "1791714000" } })), {
    kind: "source",
    source: "src.weather",
    status: { ok: true, error: null, updated: 1791714000 },
  });
  assert.deepEqual(parseLive(JSON.stringify({ source: { source: "src.x", error: "x: timed out" } })), {
    kind: "source",
    source: "src.x",
    status: { ok: false, error: "x: timed out", updated: 0 },
  });
});

test("a pushed state keeps a slider's value", () => {
  const message = parseLive(JSON.stringify({ state: { widgetId: "w", ok: true, value: 42, label: "Mic" } }));
  assert.deepEqual(message, { kind: "state", widgetId: "w", state: { value: 42, label: "Mic" } });
});
