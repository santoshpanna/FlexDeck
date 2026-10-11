import assert from "node:assert/strict";
import { test } from "node:test";

import { remaining } from "../src/modules/shell/lib/countdown.lib.ts";

test("a countdown formats as minutes and seconds", () => {
  assert.equal(remaining(1000 + 125, 1000).text, "2:05");
});

test("seconds are padded so the width does not jump", () => {
  assert.equal(remaining(1000 + 65, 1000).text, "1:05");
  assert.equal(remaining(1000 + 5, 1000).text, "0:05");
});

test("the expiry instant itself is already expired", () => {
  // Matches the agent, which compares with `>=` so the boundary is closed.
  const at = remaining(1000, 1000);
  assert.equal(at.expired, true);
  assert.equal(at.text, "0:00");
});

test("a past expiry never counts backwards", () => {
  const past = remaining(1000, 5000);
  assert.equal(past.expired, true);
  assert.equal(past.seconds, 0);
  assert.equal(past.text, "0:00");
});
