import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, test } from "node:test";

import { checkDefinition, harness, type Host } from "@flexdeck/plugin-sdk/testing";

import plugin, { findAction, forgetSwitches, parseArgs, reset, slug, useConnection } from "../src/index.ts";
import { authResponse } from "../src/sb-client.ts";
import { FakeStreamerbot } from "./fake-streamerbot.ts";

afterEach(() => {
  reset();
  forgetSwitches();
  useConnection(undefined);
});

async function until(condition: () => boolean, what = "the condition"): Promise<void> {
  const deadline = Date.now() + 3000;
  while (!condition()) {
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}

async function connected(bot: FakeStreamerbot, settings: Record<string, string> = {}): Promise<Host> {
  useConnection(bot.connect);
  plugin.configure(settings, mkdtempSync(join(tmpdir(), "flexdeck-sb-")));
  const host = await harness(plugin, { settings, start: true });
  await until(
    () => host.variable("streamerbot.connected") === true && host.variable("streamerbot.action.shout_out.enabled") !== undefined,
    "the connection",
  );
  return host;
}

test("the definition is one the agent will accept", async () => {
  await checkDefinition(plugin, new URL("../plugin.json", import.meta.url), { run: false });
  const manifest = plugin.manifest(3);
  assert.equal(manifest.actions.find((action) => action.action === "run_action_value")?.slider?.argument, "value");
});

test("arguments are read as name=value pairs", () => {
  assert.deepEqual(parseArgs("user=ada; message=hi = there ;;bad"), { user: "ada", message: "hi = there" });
  assert.deepEqual(parseArgs(undefined), {});
});

test("an action is found by id, by name, or by its variable's slug", () => {
  const actions = new FakeStreamerbot().actions;
  assert.equal(findAction(actions, "11111111-1111-1111-1111-111111111111")?.name, "Shout-out");
  assert.equal(findAction(actions, "lights on")?.name, "Lights on");
  assert.equal(findAction(actions, "shout_out")?.name, "Shout-out");
  assert.equal(slug("Scene: Live!"), "scene_live");
});

test("connecting publishes whether each action is enabled", async () => {
  const bot = new FakeStreamerbot();
  const host = await connected(bot);
  assert.equal(host.variable("streamerbot.version"), "0.2.5");
  assert.equal(host.variable("streamerbot.action.lights_off.enabled"), false);
  assert.equal(host.variable("streamerbot.action.lights_on.enabled"), true);
  assert.deepEqual(host.events().at(-1), { event: "connected", payload: {} });
});

test("a password is answered with the challenge response, and a wrong one is refused", async () => {
  const bot = new FakeStreamerbot({ password: "hunter2" });
  const host = await connected(bot, { password: "hunter2" });
  assert.equal(host.variable("streamerbot.connected"), true);
  reset();

  const wrong = new FakeStreamerbot({ password: "hunter2" });
  useConnection(wrong.connect);
  const refused = await harness(plugin, { settings: { password: "nope" }, start: true });
  await until(() => refused.logs().some((line) => line.message.includes("refused the password")), "the refusal");
  assert.equal(refused.variable("streamerbot.connected"), false);
});

test("the auth response is the one Streamer.bot computes", () => {
  // The same construction obs-websocket documents; Streamer.bot uses it too.
  assert.equal(
    authResponse("supersecretpassword", "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=", "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY="),
    "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4=",
  );
});

test("running an action sends its id and arguments, and its completion is an event", async () => {
  const bot = new FakeStreamerbot();
  const host = await connected(bot);
  await host.invoke("run_action", { action: "11111111-1111-1111-1111-111111111111", args: "user=ada" });
  assert.deepEqual(bot.ran.at(-1), { name: "Shout-out", args: { user: "ada" } });
  await until(() => host.events().some((event) => event.event === "action_completed"), "the completion");
  assert.equal(await host.check("run_action", { action: "Nope" }), "Streamer.bot has no action called 'Nope'");
});

test("a slider's value reaches the action as a number", async () => {
  const bot = new FakeStreamerbot();
  const host = await connected(bot);
  await host.invoke("run_action_value", { action: "Shout-out", argument: "volume", value: "42" });
  assert.deepEqual(bot.ran.at(-1), { name: "Shout-out", args: { volume: 42 } });
});

test("a switch alternates, reports which side it is on, and remembers across restarts", async () => {
  const bot = new FakeStreamerbot();
  const host = await connected(bot);
  const args = { key: "lights", action_a: "Lights on", action_b: "Lights off" };
  assert.equal(await host.invoke("switch", args), "a");
  assert.equal(await host.invoke("switch", args), "b");
  assert.equal(host.variable("streamerbot.switch.lights"), "b");
  assert.deepEqual(bot.ran.map((run) => run.name), ["Lights on", "Lights off"]);

  // A fresh plugin process reads the side back from its data folder.
  forgetSwitches();
  assert.equal(await host.invoke("switch", args), "a", "after b comes a, even after a restart");
});

test("switching an action on or off goes through the helper and shows at once", async () => {
  const bot = new FakeStreamerbot();
  const host = await connected(bot);
  assert.equal(await host.invoke("enable_action", { action: "Lights off" }), "enabled");
  assert.equal(host.variable("streamerbot.action.lights_off.enabled"), true);
  assert.equal(bot.actions.find((action) => action.name === "Lights off")?.enabled, true);
  assert.equal(await host.invoke("toggle_action", { action: "Lights off" }), "disabled");

  bot.actions = bot.actions.filter((action) => !action.name.startsWith("flexdeck"));
  reset();
  const without = await connected(bot);
  await assert.rejects(without.invoke("disable_action", { action: "Lights on" }), /helper action/);
});

test("mirrored globals are published, and a missing event source is not fatal", async () => {
  const bot = new FakeStreamerbot({ missingSources: ["Misc"] });
  const host = await connected(bot, { globals: "viewers, nonexistent" });
  await until(() => host.variable("streamerbot.global.viewers") === "12", "the global");
  assert.ok(host.logs().some((line) => line.message.includes("Misc")), "the refusal is logged, not thrown");
});

test("losing Streamer.bot says so, and it comes back on its own", async () => {
  const bot = new FakeStreamerbot();
  const host = await connected(bot);
  bot.quit();
  await until(() => host.variable("streamerbot.connected") === false, "the drop");
  assert.ok(host.events().some((event) => event.event === "disconnected"));
  bot.offline = false;
  await until(() => host.variable("streamerbot.connected") === true, "the reconnection");
});
