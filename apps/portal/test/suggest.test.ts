import assert from "node:assert/strict";
import { test } from "node:test";

import { applyCompletion, completions, expressionSlot, nameSlot, templateSlot, unclosedBraces } from "../src/modules/config/lib/suggest.lib.ts";

const variables = [
  { name: "obs.recording", description: "Recording" },
  { name: "obs.input.Mic.muted", description: "Mic muted" },
  { name: "obs.input.Desktop.muted", description: "Desktop muted" },
  { name: "obs.output.virtualcam", description: "Virtual cam" },
  { name: "discord.muted", description: "Muted" },
  { name: "system.cpu.usage", description: "CPU" },
];

test("nothing typed lists the first level", () => {
  assert.deepEqual(
    completions(variables, "").map((item) => item.text),
    ["discord.", "obs.", "system."],
  );
});

test("each level lists the next, branches before values", () => {
  assert.deepEqual(
    completions(variables, "obs.").map((item) => item.text),
    ["obs.input.", "obs.output.", "obs.recording"],
  );
  assert.deepEqual(
    completions(variables, "obs.in").map((item) => [item.text, item.count]),
    [["obs.input.", 2]],
  );
  assert.deepEqual(
    completions(variables, "obs.input.mic.").map((item) => [item.text, item.leaf, item.description]),
    [],
  );
  assert.deepEqual(
    completions(variables, "obs.input.Mic.").map((item) => [item.text, item.leaf, item.description]),
    [["obs.input.Mic.muted", true, "Mic muted"]],
  );
  assert.deepEqual(
    completions(variables, "obs.input.m").map((item) => item.text),
    ["obs.input.Mic."],
  );
});

test("a template offers names only inside open braces", () => {
  assert.deepEqual(templateSlot("REC {{obs.", 10), { start: 6, end: 10, query: "obs.", braces: true, open: false });
  assert.deepEqual(templateSlot("CPU {{ sys", 10)?.query, "sys");
  assert.equal(templateSlot("REC {{obs.recording}} ", 22), null);
  assert.equal(templateSlot("{{ system.cpu.usage | ro", 24), null);
  assert.equal(templateSlot("REC", 3), null);
  assert.deepEqual(templateSlot("", 0), { start: 0, end: 0, query: "", braces: true, open: true });
});

test("picking a branch goes a level deeper, picking a value closes the braces", () => {
  const text = "REC {{ob";
  const slot = templateSlot(text, text.length)!;
  const branch = applyCompletion(text, slot, { text: "obs.", leaf: false, count: 4 });
  assert.deepEqual(branch, { text: "REC {{obs.", caret: 10 });
  const leaf = applyCompletion(branch.text, templateSlot(branch.text, branch.caret)!, { text: "obs.recording", leaf: true, count: 1 });
  assert.deepEqual(leaf, { text: "REC {{obs.recording}}", caret: 21 });
});

test("a value picked mid-template keeps what follows and its braces", () => {
  const text = "{{obs.rec}} live";
  const slot = templateSlot(text, 7)!;
  assert.deepEqual(applyCompletion(text, slot, { text: "obs.recording", leaf: true, count: 1 }), { text: "{{obs.recording}} live", caret: 17 });
});

test("an empty template field adds the braces", () => {
  assert.deepEqual(applyCompletion("", templateSlot("", 0)!, { text: "obs.", leaf: false, count: 4 }), { text: "{{obs.", caret: 6 });
});

test("a name field is the name, braces dropped", () => {
  assert.equal(nameSlot("{{obs.in").query, "obs.in");
  assert.deepEqual(applyCompletion("obs.in", nameSlot("obs.in"), { text: "obs.input.", leaf: false, count: 2 }), { text: "obs.input.", caret: 10 });
});

test("a template with a {{ never closed is held back", () => {
  assert.equal(unclosedBraces("Rec {{discord.voice."), true);
  assert.equal(unclosedBraces("Rec {{a}} {{b"), true);
  assert.equal(unclosedBraces("Rec {{a}}"), false);
  assert.equal(unclosedBraces("Rec"), false);
});

test("an expression offers the name under the caret, without braces", () => {
  assert.deepEqual(expressionSlot("", 0), { start: 0, end: 0, query: "", braces: false, open: false });
  assert.deepEqual(expressionSlot("obs.rec", 7), { start: 0, end: 7, query: "obs.rec", braces: false, open: false });
  assert.deepEqual(expressionSlot("system.cpu.usage > 80 && obs.re", 31)?.query, "obs.re");
  assert.deepEqual(expressionSlot("not(obs.st", 10)?.query, "obs.st");
  assert.equal(expressionSlot("obs.recording ", 14), null);
  assert.equal(expressionSlot("x > 80", 6), null);
  assert.equal(expressionSlot('name == "obs.r', 14), null);
  assert.equal(expressionSlot("name == 'obs.r", 14), null);
  assert.equal(expressionSlot(`name == "it's obs.r`, 19), null);
  assert.equal(expressionSlot(`name == "a\\"b" && obs.r`, 23)?.query, "obs.r");
  const slot = expressionSlot("a && obs.rec || b", 12)!;
  assert.deepEqual(applyCompletion("a && obs.rec || b", slot, { text: "obs.recording", leaf: true, count: 1 }), { text: "a && obs.recording || b", caret: 18 });
});
