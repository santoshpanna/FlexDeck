import assert from "node:assert/strict";
import { mock, test } from "node:test";

import type { WSAdapter, WSAdapterSocket } from "@questorylabs/qhttp/ws";

import { isLiveFrame, LiveFeed, parseLive, type LiveMessage } from "../src/services/api/live.ts";

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

test("only a frame the desktop meant as news counts as being let in", () => {
  assert.equal(isLiveFrame(JSON.stringify({ welcome: { rev: "1", config: {} } })), true);
  assert.equal(isLiveFrame(JSON.stringify({ error: { code: "offline", message: "" } })), false);
  assert.equal(isLiveFrame(JSON.stringify({ somethingNew: {} })), false);
  assert.equal(isLiveFrame("not json"), false);
});

class FakeSocket {
  readyState = 0;
  sent: string[] = [];
  #listeners = new Map<string, Set<(event: unknown) => void>>();

  addEventListener(type: string, fn: (event: unknown) => void): void {
    let set = this.#listeners.get(type);
    if (!set) this.#listeners.set(type, (set = new Set()));
    set.add(fn);
  }

  removeEventListener(type: string, fn: (event: unknown) => void): void {
    this.#listeners.get(type)?.delete(fn);
  }

  send(data: string): void {
    this.sent.push(data);
  }

  close(code = 1000): void {
    this.drop(code);
  }

  open(): void {
    this.readyState = 1;
    this.#emit("open", {});
  }

  frame(payload: unknown): void {
    this.#emit("message", { data: JSON.stringify(payload) });
  }

  drop(code = 1005): void {
    if (this.readyState === 3) return;
    this.readyState = 3;
    this.#emit("close", { code, reason: "", wasClean: false });
  }

  #emit(type: string, event: unknown): void {
    for (const fn of [...(this.#listeners.get(type) ?? [])]) fn(event);
  }
}

class FakeAdapter implements WSAdapter {
  sockets: FakeSocket[] = [];

  connect(): WSAdapterSocket {
    const socket = new FakeSocket();
    this.sockets.push(socket);
    return socket as unknown as WSAdapterSocket;
  }

  get latest(): FakeSocket {
    return this.sockets[this.sockets.length - 1]!;
  }
}

const ROUTE = { fingerprint: "fp", name: "Desk", token: "tok", via: "relay" as const };
const HELLO = JSON.stringify({ fingerprint: "fp", token: "tok" });

function feed(onRefused: () => void = () => {}) {
  const adapter = new FakeAdapter();
  const heard: LiveMessage[] = [];
  const live = new LiveFeed(ROUTE, "https://cloud.test", "http://127.0.0.1:23540", onRefused, adapter);
  live.listen((message) => heard.push(message));
  return { adapter, heard, live };
}

async function wait(ms: number): Promise<void> {
  mock.timers.tick(ms);
  for (let i = 0; i < 5; i += 1) await new Promise((resolve) => setImmediate(resolve));
}

function refuse(socket: FakeSocket, code = "offline"): void {
  socket.open();
  socket.frame({ error: { code, message: "" } });
  socket.drop();
}

test("the hello is the first frame of every connection", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const { adapter, live } = feed();
    adapter.latest.open();
    assert.deepEqual(adapter.latest.sent, [HELLO]);
    adapter.latest.drop();
    await wait(1000);
    adapter.latest.open();
    assert.deepEqual(adapter.latest.sent, [HELLO]);
    live.close();
  } finally {
    mock.timers.reset();
  }
});

test("a desktop that keeps refusing is asked less and less often, up to 15 seconds", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const { adapter, live } = feed();
    const waits = [1000, 2000, 4000, 8000, 15_000, 15_000];
    for (const [index, ms] of waits.entries()) {
      refuse(adapter.latest);
      await wait(ms - 1);
      assert.equal(adapter.sockets.length, index + 1, `still waiting before ${ms}ms`);
      await wait(1);
      assert.equal(adapter.sockets.length, index + 2, `asked again after ${ms}ms`);
    }
    live.close();
  } finally {
    mock.timers.reset();
  }
});

test("being let in starts the waits over and tells listeners the socket is open", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const { adapter, heard, live } = feed();
    refuse(adapter.latest);
    await wait(1000);
    refuse(adapter.latest);
    await wait(2000);
    adapter.latest.open();
    adapter.latest.frame({ welcome: { rev: "3", config: {} } });
    assert.deepEqual(heard, [{ kind: "offline" }, { kind: "offline" }, { kind: "open" }, { kind: "revision", rev: 3 }]);

    adapter.latest.drop();
    await wait(1000);
    assert.equal(adapter.sockets.length, 4);
    live.close();
  } finally {
    mock.timers.reset();
  }
});

test("a desktop that drops off and comes back is told as offline, then open again", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const { adapter, heard, live } = feed();
    adapter.latest.open();
    adapter.latest.frame({ welcome: { rev: "3", config: {} } });
    adapter.latest.frame({ error: { code: "offline", message: "the desktop disconnected" } });
    adapter.latest.drop();
    await wait(1000);
    adapter.latest.open();
    adapter.latest.frame({ welcome: { rev: "4", config: {} } });
    assert.deepEqual(
      heard.map((message) => message.kind),
      ["open", "revision", "offline", "open", "revision"],
    );
    live.close();
  } finally {
    mock.timers.reset();
  }
});

test("a close without a status (1005) is reconnected", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const { adapter, live } = feed();
    adapter.latest.open();
    adapter.latest.frame({ devices: {} });
    adapter.latest.drop(1005);
    await wait(1000);
    assert.equal(adapter.sockets.length, 2);
    live.close();
  } finally {
    mock.timers.reset();
  }
});

test("a desktop that restarts with a normal close (1000, 1001) is reconnected", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const { adapter, live } = feed();
    for (const code of [1000, 1001]) {
      adapter.latest.open();
      adapter.latest.frame({ devices: {} });
      adapter.latest.drop(code);
      await wait(1000);
    }
    assert.equal(adapter.sockets.length, 3);
    live.close();
  } finally {
    mock.timers.reset();
  }
});

test("a revoked browser stops asking and is told once", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    let refusals = 0;
    const { adapter } = feed(() => {
      refusals += 1;
    });
    refuse(adapter.latest, "unauthorised");
    await wait(60_000);
    assert.equal(adapter.sockets.length, 1);
    assert.equal(refusals, 1);
  } finally {
    mock.timers.reset();
  }
});

test("a new route after a refusal connects again", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const first = feed();
    refuse(first.adapter.latest, "unauthorised");
    const second = feed();
    second.adapter.latest.open();
    assert.deepEqual(second.adapter.latest.sent, [HELLO]);
    second.live.close();
  } finally {
    mock.timers.reset();
  }
});

class ThrowingAdapter extends FakeAdapter {
  failures: number;

  constructor(failures: number) {
    super();
    this.failures = failures;
  }

  override connect(): WSAdapterSocket {
    if (this.failures > 0) {
      this.failures -= 1;
      throw new Error("blocked");
    }
    return super.connect();
  }
}

test("a socket that cannot even be made is tried again, and not after the feed is closed", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const adapter = new ThrowingAdapter(2);
    const live = new LiveFeed(ROUTE, "https://cloud.test", "http://127.0.0.1:23540", () => {}, adapter);
    assert.equal(adapter.sockets.length, 0);
    await wait(1000);
    assert.equal(adapter.sockets.length, 0);
    await wait(2000);
    assert.equal(adapter.sockets.length, 1);
    adapter.latest.open();
    assert.deepEqual(adapter.latest.sent, [HELLO]);
    live.close();

    const closed = new ThrowingAdapter(1);
    new LiveFeed(ROUTE, "https://cloud.test", "http://127.0.0.1:23540", () => {}, closed).close();
    await wait(5000);
    assert.equal(closed.sockets.length, 0);
  } finally {
    mock.timers.reset();
  }
});
