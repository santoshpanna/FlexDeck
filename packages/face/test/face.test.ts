import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { expected, type LayoutKeys } from "../scripts/gen-expected.ts";
import { LAYOUTS, SLOTS, faceFor, isLive, placeFace, placeStyle, placeZones, zoneAt, type FaceSource } from "../src/index.ts";

const DIR = fileURLToPath(new URL("../../../contract/fixtures/face/", import.meta.url));
const read = <T>(name: string): T => JSON.parse(readFileSync(`${DIR}${name}`, "utf8")) as T;

interface DeriveCase {
  name: string;
  widget: FaceSource;
  look?: string;
  face: unknown;
}

test("every derive fixture gives its face", () => {
  for (const c of read<{ cases: DeriveCase[] }>("derive.json").cases) {
    assert.deepEqual(faceFor(c.widget, c.look), c.face, c.name);
  }
});

test("a look over a widget's own face leaves the widget untouched", () => {
  const widget: FaceSource = {
    id: "w",
    face: { elements: [{ template: "CPU" }] },
    states: { looks: [{ id: "hot", label: "HOT" }] },
  };
  faceFor(widget, "hot");
  assert.equal(widget.face?.elements?.[0]?.template, "CPU");
});

test("the committed places are what the reference computes", () => {
  const input = read<LayoutKeys>("layout.json");
  assert.deepEqual(expected(input), read("layout.expected.json"));
});

test("every place of every layout lies inside the key", () => {
  for (const { w, h, unit } of read<LayoutKeys>("layout.json").keys) {
    for (const layout of LAYOUTS) {
      const places = placeFace(layout, w, h, unit);
      for (const slot of SLOTS) {
        const r = places[slot];
        assert.ok(r.x >= 0 && r.y >= 0 && r.x + r.w <= w + 0.01 && r.y + r.h <= h + 0.01, `${layout} ${slot} on ${w}x${h}`);
      }
    }
  }
});

test("a split key puts its visual beside the text when wide and above it when tall", () => {
  const wide = placeFace("FACE_LAYOUT_SPLIT", 304, 148, 1);
  assert.ok(wide.SLOT_TITLE.x > wide.SLOT_VISUAL.x + wide.SLOT_VISUAL.w - 0.01);
  const tall = placeFace("FACE_LAYOUT_SPLIT", 148, 304, 1);
  assert.ok(tall.SLOT_TITLE.y > tall.SLOT_VISUAL.y + tall.SLOT_VISUAL.h - 0.01);
});

test("a dial's value sits inside its dial", () => {
  const dial = placeFace("FACE_LAYOUT_DIAL", 148, 148, 1);
  assert.ok(dial.SLOT_VALUE.x > dial.SLOT_VISUAL.x && dial.SLOT_VALUE.w < dial.SLOT_VISUAL.w);
});

test("places draw their text sensibly when an element does not say", () => {
  assert.equal(placeStyle(undefined, "SLOT_TITLE").maxLines, 2);
  assert.equal(placeStyle("FACE_LAYOUT_VALUE", "SLOT_VALUE").size, "TEXT_SIZE_FIT");
  assert.equal(placeStyle("FACE_LAYOUT_ART", "SLOT_TITLE").align, "TEXT_ALIGN_START");
  assert.ok(placeStyle("FACE_LAYOUT_DIAL", "SLOT_CAPTION").quiet);
});

test("a template with holes is live", () => {
  assert.ok(isLive("REC {{obs.record_time}}"));
  assert.ok(!isLive("Mute"));
  assert.ok(!isLive(undefined));
});

test("a clock reads the time in its zone, in the tablet's codes", async () => {
  const { clockText, clockHands } = await import("../src/index.ts");
  const at = new Date(Date.UTC(2026, 9, 11, 21, 5, 9));
  assert.equal(clockText(at, "", "UTC", false), "21:05");
  assert.equal(clockText(at, "", "UTC", true), "21:05:09");
  assert.equal(clockText(at, "%-I:%M %p", "UTC", false), "9:05 PM");
  assert.equal(clockText(at, "%a %H:%M", "Asia/Tokyo", false), "Mon 06:05");
  assert.equal(clockText(at, "%a %-d %b", "UTC", false), "Sun 11 Oct");
  assert.equal(clockText(at, "100%% %q", "UTC", false), "100% %q", "an unknown code stays as text");
  const hands = clockHands(at, "UTC");
  assert.ok(Math.abs(hands.hour - 272.6) < 0.1 && Math.abs(hands.minute - 30.9) < 0.1 && Math.abs(hands.second - 54) < 0.1);
});

test("a strip of zones sits under the face, and a touch finds its zone", () => {
  const { face, zones } = placeZones(3, 300, 100, 1);
  assert.deepEqual(face, { x: 0, y: 0, w: 300, h: 70 });
  assert.equal(zones.length, 3);
  assert.deepEqual(zones[2], { x: 200, y: 70, w: 100, h: 30 });
  assert.equal(zoneAt(3, 300, 100, 1, 250, 90), 2);
  assert.equal(zoneAt(3, 300, 100, 1, 250, 20), -1, "above the strip: the key");
  assert.deepEqual(placeZones(0, 300, 100, 1).zones, []);
});

test("a countdown reads as the tablet shows it", async () => {
  const { countdownText, timerLeft } = await import("../src/index.ts");
  const cases = JSON.parse(readFileSync(new URL("../../../contract/fixtures/face/countdown.json", import.meta.url), "utf8")) as { cases: [number | null, string][] };
  for (const [seconds, text] of cases.cases) assert.equal(countdownText(seconds ?? undefined), text, String(seconds));
  assert.equal(timerLeft(40, true, 1_000, 11_000), 30);
  assert.equal(timerLeft(40, false, 1_000, 11_000), 40);
  assert.equal(timerLeft(5, true, 0, 60_000), 0);
});

test("an element put anywhere falls on its box, and a place turns back into one", async () => {
  const { placeBox, boxOf } = await import("../src/index.ts");
  assert.deepEqual(placeBox({ x: 0.1, y: 0.5, w: 0.5, h: 0.25 }, 200, 100), { x: 20, y: 50, w: 100, h: 25 });
  assert.deepEqual(boxOf({ x: 20, y: 50, w: 100, h: 25 }, 200, 100), { x: 0.1, y: 0.5, w: 0.5, h: 0.25 });
  // Never off the key, never vanishingly small.
  assert.deepEqual(boxOf({ x: 190, y: -5, w: 50, h: 0 }, 200, 100), { x: 0.95, y: 0, w: 0.05, h: 0.02 });
});
