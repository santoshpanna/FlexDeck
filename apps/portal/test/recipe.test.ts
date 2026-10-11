import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { RECIPE_FILE, recipeBytes } from "../scripts/gen-recipe.ts";
import { crc32, recipeFiles, recipeIdFor, recipeProblem } from "../src/modules/config/lib/recipe.lib.ts";

test("the editor's recipe package is the one the agent's tests install (run pnpm gen:recipe)", () => {
  assert.deepEqual(Buffer.from(recipeBytes()), readFileSync(RECIPE_FILE));
});

test("a zip's checksum is the standard one", () => {
  assert.equal(crc32(new TextEncoder().encode("123456789")), 0xcbf43926);
});

test("a source becomes a recipe: named after it, secrets blank, a preset that reads it", () => {
  const source = {
    id: "BTC price!",
    name: "Bitcoin",
    http: { url: "https://api.example.com/{{args.coin}}?key={{args.key}}" },
    args: [{ name: "coin", default: "bitcoin" }, { name: "key", secret: true, default: "s3cret" }],
    fields: [{ name: "price", value: "json.price" }, { name: "change", value: "json.change" }, { name: "day", value: "json.day", series: true }],
  };
  assert.equal(recipeIdFor(source), "btc-price-");
  const meta = { id: "btc", name: "Bitcoin" };
  assert.equal(recipeProblem(source, meta), null);
  const { plugin, recipe } = recipeFiles(source, meta);
  assert.equal(plugin.runtime, "recipe");
  assert.deepEqual(plugin.permissions, ["network"]);
  assert.ok(!JSON.stringify(recipe).includes("s3cret"));
  const preset = (recipe.presets as { widget: { face: { elements: { template: string }[]; background?: { graph: string } } } }[])[0]!;
  assert.deepEqual(preset.widget.face.elements.map((e) => e.template), ["Bitcoin", "{{this.price}}", "{{this.change}}"]);
  assert.equal(preset.widget.face.background?.graph, "this.day");

  assert.match(recipeProblem({ ...source, kind: "command" }, meta)!, /program/);
  assert.match(recipeProblem(source, { id: "1x", name: "B" })!, /id/);
  assert.match(recipeProblem({ ...source, fields: [] }, meta)!, /field/);
});
