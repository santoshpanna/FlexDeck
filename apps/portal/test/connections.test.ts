import assert from "node:assert/strict";
import { test } from "node:test";

import {
  forget,
  forgetVia,
  loadConnections,
  route,
  saveConnections,
  upsert,
  type Connection,
  type StorageLike,
} from "../src/services/connections/connections.lib.ts";

function memory(): StorageLike & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => void data.set(key, value),
  };
}

const local: Connection = { fingerprint: "aa", name: "Studio PC", token: "t-local", via: "local" };
const remote: Connection = { fingerprint: "aa", name: "Studio PC", token: "t-remote", via: "relay" };
const other: Connection = { fingerprint: "bb", name: "Laptop", token: "t-other", via: "relay" };

test("connections survive a reload", () => {
  const storage = memory();
  saveConnections(storage, [local, other]);
  assert.deepEqual(loadConnections(storage), [local, other]);
});

test("unreadable storage reads as no connections rather than a broken page", () => {
  const storage = memory();
  storage.setItem("flexdeck.connections", "{not json");
  assert.deepEqual(loadConnections(storage), []);

  storage.setItem("flexdeck.connections", JSON.stringify([{ fingerprint: "x" }, local]));
  assert.deepEqual(loadConnections(storage), [local], "a malformed entry is dropped, a good one kept");

  assert.deepEqual(loadConnections(null), []);
});

test("one desktop can be reached both ways, and each way is replaced on its own", () => {
  let connections = upsert(upsert([], local), remote);
  assert.equal(connections.length, 2);

  connections = upsert(connections, { ...remote, token: "t-new" });
  assert.equal(connections.length, 2);
  assert.equal(connections.find((c) => c.via === "relay")?.token, "t-new");
});

test("on this computer the direct way wins; elsewhere the cloud is used", () => {
  const connections = [remote, local];
  assert.equal(route(connections, "aa", true)?.via, "local");
  assert.equal(route(connections, "aa", false)?.via, "relay");
  assert.equal(route([local], "aa", false), null, "a local token is useless away from this computer");
  assert.equal(route(connections, "zz", true), null);
});

test("forgetting a desktop drops every way of reaching it", () => {
  assert.deepEqual(forget([local, remote, other], "aa"), [other]);
  assert.deepEqual(forgetVia([local, remote, other], "aa", "local"), [remote, other]);
});
