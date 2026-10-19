import assert from "node:assert/strict";
import { mock, test } from "node:test";

import { MockAdapter } from "@questorylabs/qhttp/testing";

import { Api, ApiError } from "../src/services/api/api.ts";
import type { Connection } from "../src/services/connections/connections.lib.ts";

const CLOUD = "https://cloud.test";
const LOCAL = "http://127.0.0.1:23540";

function api(adapter: MockAdapter, via: Connection["via"]): Api {
  const made = new Api(CLOUD, LOCAL, adapter);
  made.setRoutes(new Map([["fp", { fingerprint: "fp", name: "Desk", token: "tok", via }]]));
  return made;
}

async function refusal(promise: Promise<unknown>): Promise<ApiError> {
  try {
    await promise;
  } catch (error) {
    assert.ok(error instanceof ApiError);
    return error;
  }
  assert.fail("expected a refusal");
}

async function settle(): Promise<void> {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
}

test("the cloud's own error words reach the caller", async () => {
  const adapter = new MockAdapter().post(`${CLOUD}/api/presence`, { status: 429, body: { error: "too_many" } });
  const error = await refusal(api(adapter, "relay").presence(["fp"]));
  assert.equal(error.status, 429);
  assert.equal(error.message, "too_many");
});

test("a relayed answer passes the desktop's status and words through", async () => {
  const adapter = new MockAdapter().post(`${CLOUD}/api/relay`, {
    body: { status: 403, body: JSON.stringify({ error: "not allowed from here" }) },
  });
  const error = await refusal(api(adapter, "relay").status("fp"));
  assert.equal(error.status, 403);
  assert.equal(error.message, "not allowed from here");

  const sent = adapter.calls[0]!;
  assert.equal(sent.headers.authorization, "Bearer tok");
  assert.deepEqual(JSON.parse(String(sent.body)), { fingerprint: "fp", method: "GET", path: "/api/status", body: "" });
});

test("a relayed success is read from the desktop's body", async () => {
  const adapter = new MockAdapter().post(`${CLOUD}/api/relay`, {
    body: { status: 200, body: JSON.stringify({ devices: [] }) },
  });
  assert.deepEqual(await api(adapter, "relay").devices("fp"), { devices: [] });
});

test("a 409 asking for consent comes back as the desktop's answer", async () => {
  const answer = { needsConsent: true, permissions: ["network"] };
  const adapter = new MockAdapter().post(`${LOCAL}/api/plugins/install`, { status: 409, body: answer });
  assert.deepEqual(await api(adapter, "local").installPlugin("fp", "repo", "id", []), answer);
});

test("a local 401 tells the page the desktop refused this browser", async () => {
  const adapter = new MockAdapter().get(`${LOCAL}/api/status`, { status: 401, body: { error: "unauthorised" } });
  const made = api(adapter, "local");
  const refused: string[] = [];
  made.setOnLocalRefused((desktopId) => refused.push(desktopId));
  const error = await refusal(made.status("fp"));
  assert.equal(error.status, 401);
  assert.deepEqual(refused, ["fp"]);
});

test("a cloud that cannot be reached is status 0 with words for people", async () => {
  const adapter = new MockAdapter().post(`${CLOUD}/api/presence`, () => {
    throw new TypeError("fetch failed");
  });
  const error = await refusal(api(adapter, "relay").presence(["fp"]));
  assert.equal(error.status, 0);
  assert.equal(error.message, "the flexdeck server could not be reached");
});

test("a closed desktop app is status 0 with words for people", async () => {
  const adapter = new MockAdapter().get(`${LOCAL}/api/status`, () => {
    throw new TypeError("fetch failed");
  });
  const error = await refusal(api(adapter, "local").status("fp"));
  assert.equal(error.status, 0);
  assert.equal(error.message, "the desktop app on this computer is not running");
});

test("a relay the desktop is slow to answer says it may still finish", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const adapter = new MockAdapter().post(`${CLOUD}/api/relay`, { delay: 60_000, body: { status: 200, body: "{}" } });
    const pending = refusal(api(adapter, "relay").status("fp"));
    await settle();
    mock.timers.tick(35_000);
    const error = await pending;
    assert.equal(error.status, 0);
    assert.equal(error.message, "the desktop took too long to answer; it may still finish");
    assert.ok(error.isUnconfirmed);
  } finally {
    mock.timers.reset();
  }
});

test("a relayed write waits past the read window, then is unconfirmed rather than failed", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const adapter = new MockAdapter().post(`${CLOUD}/api/relay`, { delay: 200_000, body: { status: 200, body: "{}" } });
    let settled = false;
    const pending = refusal(api(adapter, "relay").undo("fp")).finally(() => {
      settled = true;
    });
    await settle();
    mock.timers.tick(35_000);
    await settle();
    assert.equal(settled, false, "still waiting at 35s");
    mock.timers.tick(90_000);
    const error = await pending;
    assert.equal(error.status, 0);
    assert.ok(error.isUnconfirmed);
  } finally {
    mock.timers.reset();
  }
});

test("a cloud that gave up waiting (504) is unconfirmed; an offline desktop (502) is not", async () => {
  const slow = new MockAdapter().post(`${CLOUD}/api/relay`, { status: 504, body: { error: "that desktop did not answer in time" } });
  const timedOut = await refusal(api(slow, "relay").undo("fp"));
  assert.equal(timedOut.status, 504);
  assert.ok(timedOut.isUnconfirmed);

  const gone = new MockAdapter().post(`${CLOUD}/api/relay`, { status: 502, body: { error: "that desktop is not connected" } });
  const offline = await refusal(api(gone, "relay").undo("fp"));
  assert.equal(offline.isUnconfirmed, false);
});

test("no desktop app on this computer is an ordinary null", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const adapter = new MockAdapter().get(`${LOCAL}/api/local/hello`, { delay: 60_000, body: {} });
    const pending = api(adapter, "local").localHello();
    await settle();
    mock.timers.tick(1500);
    assert.equal(await pending, null);
  } finally {
    mock.timers.reset();
  }
});

test("a desktop app on this computer says who it is", async () => {
  const adapter = new MockAdapter().get(`${LOCAL}/api/local/hello`, { body: { fingerprint: "fp", name: "Desk" } });
  assert.deepEqual(await api(adapter, "local").localHello(), { fingerprint: "fp", name: "Desk" });
});
