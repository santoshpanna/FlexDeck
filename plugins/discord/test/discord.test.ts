import assert from "node:assert/strict";
import { afterEach, test } from "node:test";

import { checkDefinition, harness, type Host } from "@flexdeck/plugin-sdk/testing";

import plugin, { reset, usePipe } from "../src/index.ts";
import { decode, encode } from "../src/ipc.ts";
import { pkce } from "../src/oauth.ts";
import { FakeDiscord, type FakeDiscordOptions } from "./fake-discord.ts";

let fake: FakeDiscord | null = null;

afterEach(async () => {
  reset();
  usePipe(undefined);
  await fake?.stop();
  fake = null;
});

async function until(condition: () => boolean, what = "the condition"): Promise<void> {
  const deadline = Date.now() + 4000;
  while (!condition()) {
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}

async function discord(options: FakeDiscordOptions = {}): Promise<FakeDiscord> {
  fake = new FakeDiscord(options);
  await fake.start();
  usePipe(fake.pipe);
  return fake;
}

async function signedIn(fakeDiscord: FakeDiscord, settings: Record<string, string> = {}): Promise<Host> {
  const host = await harness(plugin, {
    settings: { client_id: "123", api_base: fakeDiscord.apiBase, ...settings },
    start: true,
  });
  await until(() => host.variable("discord.authorized") === true && host.variable("discord.voice.users") !== undefined, "signing in");
  return host;
}

test("the definition is one the agent will accept, with sliders for every volume", async () => {
  await checkDefinition(plugin, new URL("../plugin.json", import.meta.url), { run: false });
  const sliders = plugin
    .manifest(3)
    .actions.filter((action) => action.slider)
    .map((action) => `${action.action}:${action.slider!.max}`);
  assert.deepEqual(sliders, ["set_input_volume:100", "set_output_volume:200", "set_user_volume:200"]);
});

test("frames are an op, a length and JSON, and split frames are reassembled", () => {
  const bytes = Buffer.concat([encode(1, { cmd: "A" }), encode(3, { ping: true })]);
  const half = decode(bytes.subarray(0, 10));
  assert.equal(half.frames.length, 0, "an incomplete frame waits");
  const all = decode(bytes);
  assert.deepEqual(
    all.frames.map((frame) => [frame.op, frame.payload]),
    [
      [1, { cmd: "A" }],
      [3, { ping: true }],
    ],
  );
  assert.equal(all.rest.length, 0);
});

test("a PKCE challenge is the hash of its verifier", async () => {
  const { createHash } = await import("node:crypto");
  const { verifier, challenge } = pkce();
  assert.equal(createHash("sha256").update(verifier).digest("base64url"), challenge);
  assert.ok(verifier.length >= 43, "RFC 7636's minimum length");
});

test("with no Client ID it says what to do instead of failing obscurely", async () => {
  await discord();
  const host = await harness(plugin, { settings: { client_id: "" }, start: true });
  await until(() => host.logs().some((line) => line.message.includes("Client ID")), "the guidance");
  assert.equal(host.variable("discord.authorized"), false);
});

test("first sign-in asks Discord, exchanges the code with PKCE, and keeps the tokens", async () => {
  const fakeDiscord = await discord();
  const host = await signedIn(fakeDiscord);
  const authorize = fakeDiscord.commands.find((command) => command.cmd === "AUTHORIZE");
  assert.equal(authorize?.args.code_challenge_method, "S256", "no secret, so PKCE");
  assert.ok((authorize?.args.scopes as string[]).includes("rpc.voice.write"));
  assert.ok(fakeDiscord.tokenRequests[0]?.code_verifier, "the verifier proves who started it");
  assert.equal(fakeDiscord.tokenRequests[0]?.client_secret, undefined, "no secret is sent, because none exists");
  assert.equal(host.stored().refresh_token, "refresh-1");
  assert.equal(host.variable("discord.voice.input_volume"), 80);
  assert.equal(host.variable("discord.voice.channel"), "General");
  assert.equal(host.variable("discord.voice.guild"), "Test Server");
  assert.equal(host.variable("discord.voice.users"), 2);
});

test("your own application's secret is used, and only when you gave one", async () => {
  const fakeDiscord = await discord({ requireSecret: true });
  await signedIn(fakeDiscord, { client_secret: "s3cret" });
  assert.equal(fakeDiscord.tokenRequests[0]?.client_secret, "s3cret");
  const authorize = fakeDiscord.commands.find((command) => command.cmd === "AUTHORIZE");
  assert.equal(authorize?.args.code_challenge, undefined, "a confidential app does not need PKCE");
});

test("a saved sign-in is used again, and an expired one is refreshed without asking", async () => {
  const fakeDiscord = await discord();
  const later = String(Date.now() + 3_600_000);
  await signedIn(fakeDiscord, { access_token: "access-7", refresh_token: "refresh-7", expires_at: later });
  assert.ok(!fakeDiscord.commands.some((command) => command.cmd === "AUTHORIZE"), "no dialog for a good token");
  reset();
  await fakeDiscord.stop();

  const again = await discord();
  const host = await signedIn(again, { access_token: "access-old", refresh_token: "refresh-7", expires_at: "1" });
  assert.equal(again.tokenRequests[0]?.grant_type, "refresh_token");
  assert.ok(!again.commands.some((command) => command.cmd === "AUTHORIZE"), "refreshed, not re-asked");
  assert.equal(host.stored().refresh_token, "refresh-1", "Discord's rotated refresh token is kept");
});

test("permission refused in Discord is said plainly and not asked again on a timer", async () => {
  const fakeDiscord = await discord({ deny: true });
  const host = await harness(plugin, { settings: { client_id: "123", api_base: fakeDiscord.apiBase }, start: true });
  await until(() => host.logs().some((line) => line.message.includes("permission was not given")), "the refusal");
  const asked = fakeDiscord.commands.filter((command) => command.cmd === "AUTHORIZE").length;
  await new Promise((resolve) => setTimeout(resolve, 1300));
  assert.equal(fakeDiscord.commands.filter((command) => command.cmd === "AUTHORIZE").length, asked, "no dialog loop");
});

test("muting is a toggle that reports its look, and the state comes back as an event", async () => {
  const fakeDiscord = await discord();
  const host = await signedIn(fakeDiscord);
  assert.equal(await host.invoke("toggle_mute"), "muted");
  assert.equal(fakeDiscord.voice.mute, true);
  await until(() => host.variable("discord.voice.mute") === true, "the mute variable");
  assert.equal(await host.invoke("toggle_mute"), "live");
});

test("push-to-talk opens the microphone on press and closes it on release", async () => {
  const fakeDiscord = await discord();
  const host = await signedIn(fakeDiscord);
  await host.invoke("ptt_start");
  assert.equal(fakeDiscord.voice.mute, false);
  await host.invoke("ptt_end");
  assert.equal(fakeDiscord.voice.mute, true);
});

test("volumes, devices and the voice mode are set the way Discord wants them", async () => {
  const fakeDiscord = await discord();
  const host = await signedIn(fakeDiscord);
  await host.invoke("set_output_volume", { volume: "150" });
  assert.equal(fakeDiscord.voice.output.volume, 150);
  await host.invoke("set_input_device", { device: "yeti" });
  assert.equal(fakeDiscord.voice.input.device_id, "mic-2", "a device by part of its name");
  await until(() => host.variable("discord.voice.input_device") === "Microphone (Yeti)", "the device variable");
  assert.equal(await host.invoke("toggle_voice_mode"), "ptt");
  await assert.rejects(host.invoke("set_output_volume", { volume: "250" }), /0 to 200/);
});

test("one person's volume is set by their id, and its variable follows for a slider", async () => {
  const fakeDiscord = await discord();
  const host = await signedIn(fakeDiscord);
  const choices = await host.choices("set_user_volume");
  assert.deepEqual(
    choices.arguments.user?.map((choice) => choice.label),
    ["Me", "Ada"],
    "people in the call, by the name Discord shows",
  );
  await host.invoke("set_user_volume", { user: "2", volume: "40" });
  assert.equal(fakeDiscord.members.get("2")?.volume, 40);
  await until(() => host.variable("discord.user.2.volume") === 40, "the slider's feedback");
});

test("joining another channel moves the call and re-follows its people", async () => {
  const fakeDiscord = await discord();
  const host = await signedIn(fakeDiscord);
  const channels = await host.choices("join_voice");
  assert.deepEqual(
    channels.arguments.channel?.map((choice) => choice.label),
    ["Test Server / General", "Test Server / Gaming"],
  );
  await host.invoke("join_voice", { channel: "201" });
  await until(() => host.variable("discord.voice.channel") === "Gaming", "the new channel");
  assert.ok(host.events().some((event) => event.event === "voice_channel_changed"));
  await host.invoke("leave_voice");
  await until(() => host.variable("discord.voice.channel") === "", "leaving");
});

test("someone speaking and joining shows up, and notifications are counted", async () => {
  const fakeDiscord = await discord();
  const host = await signedIn(fakeDiscord);
  fakeDiscord.dispatch("SPEAKING_START", { user_id: "2", channel_id: "200" });
  await until(() => host.variable("discord.user.2.speaking") === true, "Ada speaking");
  fakeDiscord.dispatch("VOICE_STATE_CREATE", { user: { id: "3", username: "grace" }, nick: "Grace", volume: 100, mute: false });
  await until(() => host.variable("discord.voice.users") === 3, "Grace joining");
  assert.ok(host.events().some((event) => event.event === "user_joined" && event.payload.name === "Grace"));
  fakeDiscord.dispatch("NOTIFICATION_CREATE", { channel_id: "300", title: "Ada", body: "hi" });
  await until(() => host.variable("discord.notifications.count") === 1, "the notification");
  await host.invoke("clear_notifications");
  assert.equal(host.variable("discord.notifications.count"), 0);
});

test("a command this Discord does not offer is detected and said plainly", async () => {
  const fakeDiscord = await discord({ unknown: ["TOGGLE_VIDEO"] });
  const host = await signedIn(fakeDiscord);
  await assert.rejects(host.invoke("toggle_video"), /does not let apps do that/);
  assert.equal(await host.check("toggle_video"), "this Discord does not let apps turn the camera on or off");
  await host.invoke("toggle_screenshare");
  await host.invoke("play_sound", { sound: "100:900" });
  assert.deepEqual(fakeDiscord.commands.at(-1), { cmd: "PLAY_SOUNDBOARD_SOUND", args: { guild_id: "100", sound_id: "900" } });
});

test("server stats count voice channels and who is in them", async () => {
  const fakeDiscord = await discord();
  const host = await signedIn(fakeDiscord, { stats_guild: "Test Server" });
  const values = await host.sample(["discord.stats.voice_channels", "discord.stats.in_voice", "discord.stats.guild_name"]);
  assert.deepEqual(values, {
    "discord.stats.voice_channels": 2,
    "discord.stats.in_voice": 2,
    "discord.stats.guild_name": "Test Server",
  });
});

test("Discord quitting is noticed, and the plugin reconnects when it is back", async () => {
  const fakeDiscord = await discord();
  const host = await signedIn(fakeDiscord);
  fakeDiscord.quit();
  await until(() => host.variable("discord.connected") === false, "the drop");
  assert.ok(host.events().some((event) => event.event === "disconnected"));
  await until(() => host.variable("discord.connected") === true && host.variable("discord.authorized") === true, "coming back");
});

test("a Discord that refuses the handshake is the answer, not a reason to try other pipes", async () => {
  const fakeDiscord = await discord();
  const { DiscordIpc } = await import("../src/ipc.ts");
  // The fake refuses an empty Client ID the way the real Discord refuses an
  // unknown one: close code 4000 on the handshake.
  await assert.rejects(new DiscordIpc({ clientId: "", path: fakeDiscord.pipe }).open(), /does not know that Client ID/);
});
