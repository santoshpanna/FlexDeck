import assert from "node:assert/strict";
import { test } from "node:test";

import { statusRows, webAccessSummary } from "../src/modules/shell/lib/status.lib.ts";
import type { Status } from "../src/modules/shell/lib/types.ts";

function status(overrides: Partial<Status["tunnel"]> = {}, warnings: string[] = []): Status {
  return {
    fingerprint: "abc",
    host: "192.168.1.10",
    port: 7878,
    warnings,
    server: { error: null },
    mcp: { url: "http://127.0.0.1:7879/mcp", error: null },
    tunnel: {
      enabled: false,
      running: false,
      refused: null,
      lastError: null,
      portal: null,
      ...overrides,
    },
    devices: [],
    plugins: { builtIn: [], installed: [] },
    pairing: null,
    browserPairing: null,
    approvals: [],
  };
}

function find(rows: ReturnType<typeof statusRows>, label: string) {
  const row = rows.find((candidate) => candidate.label === label);
  assert.ok(row, `expected a "${label}" row`);
  return row;
}

test("a listener that never came up is reported, not hidden", () => {
  // Everything else in the window can be healthy while nothing outside this
  // machine can reach it — a taken port is the usual cause, and it is invisible
  // from inside the process.
  const blocked = status();
  blocked.server = { error: "Something is already using port 7878." };

  const row = find(statusRows(blocked), "This computer");

  assert.equal(row.tone, "warn");
  assert.match(row.detail, /7878/);
});

test("the server being up is its own row", () => {
  const row = find(statusRows(status()), "This computer");
  assert.equal(row.tone, "ok");
  assert.match(row.detail, /192\.168\.1\.10:7878/);
});

test("no portal address reads as a thing to fill in, not as broken", () => {
  const row = find(statusRows(status()), "Web access");

  assert.equal(row.tone, "off");
  assert.match(row.detail, /No portal address set/);
});

test("the card asks for an address before offering the switch", () => {
  assert.match(webAccessSummary(status().tunnel), /Set the portal address/);
});

test("connected is the good state", () => {
  const row = find(statusRows(status({ enabled: true, running: true, portal: "wss://p.example/t" })), "Web access");

  assert.equal(row.tone, "ok");
  assert.match(row.detail, /p\.example/);
});

test("enabled but not running is reported honestly rather than as connected", () => {
  // This is the "comforting lie" the agent's own code refuses to tell.
  const row = find(statusRows(status({ enabled: true, running: false, portal: "wss://p.example/t" })), "Web access");

  assert.equal(row.tone, "warn");
  assert.match(row.detail, /could not be reached/);
});

test("a refusal is reported as a refusal, not as unreachable", () => {
  // The portal is up and answering — it is refusing. Calling this "could not be
  // reached" would send the user to check a network that is working.
  const row = find(
    statusRows(
      status({
        enabled: true,
        running: true,
        portal: "wss://p.example/t",
        refused: { code: "protocol_version", message: "this server speaks tunnel protocol 3" },
      }),
    ),
    "Web access",
  );

  assert.equal(row.tone, "warn");
  assert.match(row.detail, /refused/);
});

test("the card must not claim the website can reach a computer it cannot reach", () => {
  const summary = webAccessSummary(status({ enabled: true, running: false, portal: "wss://p.example/t" }).tunnel);

  assert.match(summary, /cannot reach this computer/);
});

test("a refusal on the card names the remedy", () => {
  const summary = webAccessSummary(
    status({
      enabled: true,
      portal: "wss://p.example/t",
      refused: { code: "protocol_version", message: "speaks 3, sent 2" },
    }).tunnel,
  );

  assert.match(summary, /refused/);
  assert.match(summary, /Update this app/);
});

test("every warning becomes a row of its own", () => {
  // The warnings describe problems that are invisible from this machine, so
  // dropping one would hide exactly the failure the user cannot otherwise see.
  const rows = statusRows(status({}, ["the address is not reachable", "presses do nothing"]));

  const warnings = rows.filter((row) => row.label === "Needs attention");
  assert.equal(warnings.length, 2);
  assert.ok(warnings.every((row) => row.tone === "warn"));
});

test("there is no account to link, so no link row ever appears", () => {
  const rows = statusRows(status({ enabled: true, running: true, portal: "wss://p.example/t" }));
  assert.equal(rows.find((row) => /link/i.test(row.label)), undefined);
});
