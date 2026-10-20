import assert from "node:assert/strict";
import { test } from "node:test";

import { WriteQueue } from "../src/modules/config/lib/write.queue.lib.ts";

interface Doc {
  rev?: number;
  name: string;
}

function agentAt(head: number) {
  const sent: Doc[] = [];
  const release: (() => void)[] = [];
  const state = { head };
  const send = (document: Doc) => {
    sent.push(document);
    return new Promise<{ rev: number }>((resolve, reject) => {
      release.push(() => {
        if (document.rev !== undefined && document.rev !== state.head) {
          reject(Object.assign(new Error("conflict"), { status: 409 }));
          return;
        }
        state.head += 1;
        resolve({ rev: state.head });
      });
    });
  };
  const next = async () => {
    while (release.length === 0) await new Promise((resolve) => setImmediate(resolve));
    release.shift()!();
  };
  return { sent, send, state, next };
}

test("quick edits built on the document being written go out one at a time, each from the last one's revision", async () => {
  const agent = agentAt(5);
  const queue = new WriteQueue<Doc>(agent.send);
  const first = queue.push({ rev: 5, name: "a" });
  const second = queue.push({ rev: 5, name: "ab" });
  const third = queue.push({ rev: 5, name: "abc" });
  assert.equal(queue.busy, true);
  assert.equal(agent.sent.length, 0);

  await agent.next();
  assert.equal(await first, true);
  await agent.next();
  assert.equal(await second, true);
  await agent.next();
  assert.equal(await third, true);

  assert.deepEqual(agent.sent.map((doc) => doc.rev), [5, 6, 7]);
  assert.equal(agent.state.head, 8);
  assert.equal(queue.busy, false);
});

test("a write from a fresh read is sent as read", async () => {
  const agent = agentAt(5);
  const queue = new WriteQueue<Doc>(agent.send);
  const first = queue.push({ rev: 5, name: "a" });
  await agent.next();
  await first;
  queue.settle();

  agent.state.head = 9;
  const fresh = queue.push({ rev: 9, name: "b" });
  await agent.next();
  assert.equal(await fresh, true);
  assert.equal(agent.sent[1]!.rev, 9);
});

test("after settling, an edit from the old read is not moved onto the new revision", async () => {
  const agent = agentAt(5);
  const queue = new WriteQueue<Doc>(agent.send);
  const first = queue.push({ rev: 5, name: "a" });
  await agent.next();
  await first;
  queue.settle();

  const stale = queue.push({ rev: 5, name: "old" });
  await agent.next();
  await assert.rejects(stale, /conflict/);
});

test("a write someone else beat is refused, and the edits queued behind it are dropped", async () => {
  const agent = agentAt(5);
  const queue = new WriteQueue<Doc>(agent.send);
  agent.state.head = 6;
  const first = queue.push({ rev: 5, name: "a" });
  const second = queue.push({ rev: 5, name: "ab" });
  await agent.next();
  await assert.rejects(first, /conflict/);
  assert.equal(await second, false);
  assert.equal(agent.sent.length, 1);
  assert.equal(queue.busy, false);

  const retried = queue.push({ rev: 6, name: "a" });
  await agent.next();
  assert.equal(await retried, true);
});

test("idle waits for every write pushed so far", async () => {
  const agent = agentAt(1);
  const queue = new WriteQueue<Doc>(agent.send);
  let done = false;
  void queue.push({ rev: 1, name: "a" });
  const idle = queue.idle().then(() => {
    done = true;
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(done, false);
  await agent.next();
  await idle;
  assert.equal(done, true);
});
