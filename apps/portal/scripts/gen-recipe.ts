/**
 * Write a recipe package made the way the editor makes one, where the
 * agent's tests install it:
 *
 *   pnpm gen:recipe
 *
 * So the editor's zip and recipe, and the agent's reading of them, cannot
 * drift apart unnoticed.
 */
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { packRecipe } from "../src/modules/config/lib/recipe.lib.ts";
import { STARTERS } from "../src/modules/config/lib/sources.lib.ts";

export const RECIPE_FILE = fileURLToPath(new URL("../../../contract/fixtures/plugin/recipe-from-editor.fdpkg", import.meta.url));

export function recipeBytes(): Uint8Array {
  const weather = STARTERS.find((starter) => starter.label === "Weather")!;
  return packRecipe({ id: "weather", ...weather.source, args: [...(weather.source.args ?? []), { name: "key", secret: true, default: "never-shared" }] }, {
    id: "my-weather",
    name: "My weather",
    description: "Made in the Data tab.",
    author: "me",
  });
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  writeFileSync(RECIPE_FILE, recipeBytes());
  console.log("wrote recipe-from-editor.fdpkg");
}
