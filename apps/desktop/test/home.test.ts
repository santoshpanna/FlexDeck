import assert from "node:assert/strict";
import { test } from "node:test";

import { editorUrl, newTablet, nextStep, readouts } from "../src/modules/shell/lib/home.lib.ts";
import type { DeviceRow, Status } from "../src/modules/shell/lib/types.ts";

function status(patch: Partial<Status> = {}): Status {
  return {
    fingerprint: "abcd",
    host: "192.168.1.24",
    port: 7878,
    warnings: [],
    server: { error: null },
    tunnel: { enabled: false, running: false, refused: null, lastError: null, portal: null },
    devices: [],
    plugins: { builtIn: [], installed: [] },
    pairing: null,
    browserPairing: null,
    approvals: [],
    ...patch,
  };
}

function tablet(id: string, online: boolean, created_at = 1): DeviceRow {
  return { id, name: `Tab ${id}`, created_at, online, kind: "tablet" };
}

test("a blocked server comes before everything", () => {
  const step = nextStep(status({ server: { error: "port 7878 is taken" }, devices: [tablet("a", true)] }));
  assert.deepEqual(step, { kind: "blocked", detail: "port 7878 is taken" });
});

test("no tablet means pair one", () => {
  assert.equal(nextStep(status()).kind, "pair");
});

test("browsers do not count as tablets", () => {
  const browser: DeviceRow = { id: "b", name: "Chrome", created_at: 1, online: true, kind: "browser-local" };
  assert.equal(nextStep(status({ devices: [browser] })).kind, "pair");
});

test("tablets all offline means wake the newest", () => {
  const step = nextStep(status({ devices: [tablet("old", false, 1), tablet("new", false, 5)] }));
  assert.deepEqual(step, { kind: "wake", tablet: "Tab new" });
});

test("a live tablet means build, with the editor link when there is a portal", () => {
  const step = nextStep(
    status({
      devices: [tablet("a", true)],
      tunnel: { enabled: true, running: true, refused: null, lastError: null, portal: "wss://deck.example.net/tunnel" },
    }),
  );
  assert.deepEqual(step, { kind: "build", editor: "https://deck.example.net/config" });
});

test("editorUrl maps tunnel schemes and refuses what it cannot read", () => {
  assert.equal(editorUrl("ws://127.0.0.1:8787/tunnel"), "http://127.0.0.1:8787/config");
  assert.equal(editorUrl("https://deck.example.net"), "https://deck.example.net/config");
  assert.equal(editorUrl(null), null);
  assert.equal(editorUrl("not a url"), null);
  assert.equal(editorUrl("ftp://x"), null);
});

test("readouts keep each fact separate", () => {
  const rows = readouts(
    status({
      devices: [tablet("a", true), tablet("b", false)],
      tunnel: { enabled: true, running: false, refused: null, lastError: null, portal: "wss://x" },
    }),
  );
  assert.deepEqual(
    rows.map((row) => [row.label, row.value, row.led]),
    [
      ["Agent", "Online", "ok"],
      ["Tablets", "1/2 live", "ok"],
      ["Web", "Trying", "wait"],
    ],
  );
});

test("newTablet finds only a tablet that was not there before", () => {
  const devices = [tablet("a", true), tablet("b", true)];
  assert.equal(newTablet(["a"], devices)?.id, "b");
  assert.equal(newTablet(["a", "b"], devices), null);
});
