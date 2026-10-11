import assert from "node:assert/strict";
import { test } from "node:test";

import { checkDefinition, harness } from "@flexdeck/plugin-sdk/testing";

import plugin from "../src/index.ts";

test("the definition is one the agent will accept", async () => {
  // Every action runs with its own examples, ids and namespaces are valid,
  // and plugin.json agrees with the code.
  await checkDefinition(plugin, new URL("../plugin.json", import.meta.url));
});

test("greeting counts, publishes, announces, and notifies", async () => {
  const host = await harness(plugin, { settings: { greeting: "Hi" } });
  // The plugin keeps its count for as long as it runs, and other tests in
  // this file have greeted too — so compare with what it was.
  const before = Number(host.variable("template.greetings") ?? 0);

  await host.invoke("greet", { name: "ada" });

  assert.equal(host.variable("template.greetings"), before + 1);
  assert.deepEqual(host.events(), [{ event: "greeted", payload: { name: "ada" } }]);
  assert.deepEqual(host.notifications(), [{ title: "Hi", body: "ada" }]);
});

test("a blank name is refused before anything runs", async () => {
  const host = await harness(plugin);

  assert.equal(await host.check("greet", { name: "  " }), "a name is required");
});
