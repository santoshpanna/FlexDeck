import assert from "node:assert/strict";
import { test } from "node:test";

import { decode, decodeDuration, project, render } from "../src/ascii/engine.ts";
import { FIELDS } from "../src/ascii/fields.ts";
import { RocketSim } from "../src/ascii/rocket.ts";

function shape(text: string): number[] {
  return text.split("\n").slice(0, -1).map((line) => line.length);
}

test("a frame is cols × rows in both layers", () => {
  const frame = render({ cols: 40, rows: 16, yaw: 0.6 });
  assert.deepEqual(shape(frame.base), Array(16).fill(40));
  assert.deepEqual(shape(frame.glow), Array(16).fill(40));
});

test("a cell is drawn in at most one layer", () => {
  const lit = Array(15).fill(1);
  const frame = render({ cols: 60, rows: 24, yaw: 0.4, lit });
  const base = frame.base;
  const glow = frame.glow;
  for (let i = 0; i < base.length; i++) {
    if (glow[i] !== " " && glow[i] !== "\n") assert.equal(base[i], " ", `cell ${i} is in both layers`);
  }
});

test("the deck is actually in the picture", () => {
  const frame = render({ cols: 100, rows: 40, yaw: 0.6, floor: false });
  const inked = frame.base.replace(/[\s.*]/g, "").length;
  assert.ok(inked > 400, `only ${inked} solid cells — the model is missing or off-frame`);
});

test("lighting every key moves cells into the glow layer", () => {
  const dark = render({ cols: 60, rows: 24, yaw: 0.6 });
  const lit = render({ cols: 60, rows: 24, yaw: 0.6, lit: Array(15).fill(1) });
  assert.equal(dark.glow.trim(), "");
  assert.ok(lit.glow.trim().length > 20);
});

test("rendering is deterministic for the same inputs", () => {
  const options = { cols: 50, rows: 20, yaw: 1.1, explode: 0.4, t: 2 };
  assert.deepEqual(render(options), render(options));
});

test("the centre of the model projects near the centre of the frame", () => {
  const point = project({ cols: 80, rows: 40, yaw: 0.6, elev: 0.4 }, 0, 0, 0);
  assert.ok(Math.abs(point.x - 0.5) < 0.05, `x ${point.x}`);
  assert.ok(Math.abs(point.y - 0.5) < 0.1, `y ${point.y}`);
});

test("decode keeps layout and settles on the text", () => {
  const text = "Every key,\nexactly yours.";
  const early = decode(text, 0);
  assert.equal(early.length, text.length);
  assert.equal(early.indexOf("\n"), text.indexOf("\n"));
  assert.equal(decode(text, decodeDuration(text) + 0.01), text);
});

test("every field draws the size it was asked for", () => {
  for (const [name, field] of Object.entries(FIELDS)) {
    const frame = field(1.5, 30, 9);
    assert.deepEqual(shape(frame.base), Array(9).fill(30), name);
  }
});

test("a held fader lights up, and lets go", () => {
  const lit = (t: number) => FIELDS.faders(t).glow.replace(/[\s=]/g, "").length;
  assert.ok(lit(1.5) > lit(5) + 10, `held ${lit(1.5)}, at rest ${lit(5)}`);
});

test("the rocket sim is stable over many frames", () => {
  let seed = 1;
  const random = () => {
    seed = (seed * 16807) % 2147483647;
    return seed / 2147483647;
  };
  const sim = new RocketSim(80, 40, random);
  let frame = sim.step(0, 0);
  for (let i = 1; i < 200; i++) frame = sim.step(i / 30, Math.min(1, i / 150));
  assert.deepEqual(shape(frame.base), Array(40).fill(80));
});
