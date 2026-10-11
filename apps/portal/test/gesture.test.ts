import assert from "node:assert/strict";
import { test } from "node:test";

import { swipeDirection } from "../src/modules/config/lib/gesture.lib.ts";

/**
 * Swipe recognition.
 *
 * The two cases that matter are the ones that must NOT change the page: a
 * mostly-vertical drag (someone scrolling, or dragging a widget) and a small
 * horizontal jitter.
 */

test("a long, mostly-horizontal drag is a swipe", () => {
  assert.equal(swipeDirection(-120, 8), "left");
  assert.equal(swipeDirection(120, 8), "right");
});

test("a short horizontal movement is not a swipe", () => {
  assert.equal(swipeDirection(-30, 0), null);
  assert.equal(swipeDirection(59, 0), null);
});

test("a mostly-vertical drag is not a swipe, however far it goes", () => {
  assert.equal(swipeDirection(-100, 200), null);
  assert.equal(swipeDirection(100, 140), null);
});

test("a diagonal is only a swipe when it is strictly more horizontal", () => {
  // Exactly diagonal is ambiguous, so it is refused rather than guessed.
  assert.equal(swipeDirection(100, 100), null);
  assert.equal(swipeDirection(100, 99), "right");
  assert.equal(swipeDirection(-100, 99), "left");
});

test("the threshold is a parameter, so a caller can tune it", () => {
  assert.equal(swipeDirection(30, 0, 20), "right");
  assert.equal(swipeDirection(30, 0, 40), null);
});
