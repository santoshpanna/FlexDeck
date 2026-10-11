import assert from "node:assert/strict";
import { afterEach, test } from "node:test";

import { checkDefinition, harness, type Host } from "@flexdeck/plugin-sdk/testing";

import plugin, { playBody, reset } from "../src/index.ts";
import { FakeSpotify } from "./fake-spotify.ts";

let fake: FakeSpotify | null = null;

afterEach(async () => {
  reset();
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

async function spotify(): Promise<FakeSpotify> {
  fake = new FakeSpotify();
  await fake.start();
  return fake;
}

const settings = (server: FakeSpotify, extra: Record<string, string> = {}) => ({
  client_id: "abc",
  redirect_port: "0",
  accounts_base: server.base,
  api_base: server.base,
  ...extra,
});

async function signedIn(server: FakeSpotify): Promise<Host> {
  const host = await harness(plugin, { settings: settings(server, { refresh_token: "refresh-0" }), start: true });
  await until(() => host.variable("spotify.connected") === true, "signing in");
  return host;
}

test("the definition is one the agent will accept, with a slider on the volume", async () => {
  await checkDefinition(plugin, new URL("../plugin.json", import.meta.url), { run: false });
  assert.equal(plugin.manifest(3).actions.find((action) => action.action === "set_volume")?.slider?.feedback, "spotify.volume");
});

test("Spotify links and URIs become the play request Spotify wants", () => {
  assert.deepEqual(playBody("spotify:track:abc123"), { uris: ["spotify:track:abc123"] });
  assert.deepEqual(playBody("https://open.spotify.com/playlist/37i9dQ?si=x"), { context_uri: "spotify:playlist:37i9dQ" });
  assert.throws(() => playBody("https://example.com/song"), /not a Spotify link/);
});

test("connecting opens the consent page and, when the browser comes back, keeps the token", async () => {
  const server = await spotify();
  const host = await harness(plugin, { settings: settings(server), start: true });
  const signing = host.invoke("connect");
  await until(() => host.openedUrls().length === 1, "the browser to open");
  const authorize = new URL(host.openedUrls()[0]!);
  assert.equal(authorize.searchParams.get("code_challenge_method"), "S256", "PKCE, no secret");
  assert.match(authorize.searchParams.get("redirect_uri")!, /^http:\/\/127\.0\.0\.1:\d+\/callback$/, "loopback spelled out");
  // Be the browser: Spotify redirects back with a code and the state it was given.
  const redirect = new URL(authorize.searchParams.get("redirect_uri")!);
  redirect.search = new URLSearchParams({ code: "the-code", state: authorize.searchParams.get("state")! }).toString();
  const page = await fetch(redirect);
  assert.match(await page.text(), /connected/);
  await signing;
  assert.ok(server.tokenRequests[0]?.code_verifier, "the verifier proves who started it");
  assert.equal(server.tokenRequests[0]?.client_secret, undefined, "there is no secret");
  assert.equal(host.stored().refresh_token, "refresh-1");
  assert.equal(host.variable("spotify.premium"), true);
});

test("a forged answer at the loopback is refused", async () => {
  const server = await spotify();
  const host = await harness(plugin, { settings: settings(server), start: true });
  const signing = host.invoke("connect").catch((error: Error) => error);
  await until(() => host.openedUrls().length === 1, "the browser to open");
  const redirect = new URL(new URL(host.openedUrls()[0]!).searchParams.get("redirect_uri")!);
  redirect.search = "code=the-code&state=wrong";
  await fetch(redirect);
  assert.match(String(await signing), /did not match/);
  assert.equal(server.tokenRequests.length, 0, "nothing was exchanged");
});

test("the player is published with its album art, read at most every few seconds", async () => {
  const server = await spotify();
  const host = await signedIn(server);
  await host.sample(["spotify.track", "spotify.liked", "spotify.art"]);
  assert.equal(host.variable("spotify.track"), "Song");
  assert.equal(host.variable("spotify.artist"), "Ada, Grace");
  assert.equal(host.variable("spotify.liked"), false);
  assert.match(String(host.variable("spotify.art")), /^img:[0-9a-f]{64}$/);
  assert.equal(host.images()[0]?.mime, "image/jpeg", "the 300px cover, not the 640px one");
  const polls = server.calls.filter((call) => call === "GET /v1/me/player").length;
  await host.sample(["spotify.track"]);
  assert.equal(server.calls.filter((call) => call === "GET /v1/me/player").length, polls, "not again within 3s");
});

test("liking is a toggle that reports its look", async () => {
  const server = await spotify();
  const host = await signedIn(server);
  assert.equal(await host.invoke("toggle_like"), "liked");
  assert.ok(server.liked.has("t1"));
  assert.equal(await host.invoke("toggle_like"), "unliked");
  await host.invoke("add_to_playlist", { playlist: "pl1" });
  assert.deepEqual(server.added, [{ playlist: "pl1", uris: ["spotify:track:t1"] }]);
  const playlists = await host.choices("add_to_playlist");
  assert.deepEqual(playlists.arguments.playlist, [{ value: "pl1", label: "Road trip" }]);
});

test("an expired token is refreshed once, and Spotify's rotated refresh token is kept", async () => {
  const server = await spotify();
  const host = await signedIn(server);
  server.expireNext = true;
  await host.invoke("set_volume", { volume: "70" });
  assert.equal(server.volume, 70);
  assert.equal(server.tokenRequests.filter((request) => request.grant_type === "refresh_token").length, 2);
  assert.equal(host.stored().refresh_token, "refresh-2");
});

test("being told to slow down is respected, and Premium-only commands say so", async () => {
  const server = await spotify();
  const host = await signedIn(server);
  server.throttle = 1;
  await assert.rejects(host.invoke("set_volume", { volume: "10" }), /slow down/);
  await assert.rejects(host.invoke("set_volume", { volume: "10" }), /slow down/, "not even asked again yet");
  assert.equal(server.calls.filter((call) => call.startsWith("PUT /v1/me/player/volume")).length, 1);
  reset();
  await server.stop();

  const free = await spotify();
  free.premium = false;
  const host2 = await signedIn(free);
  await assert.rejects(host2.invoke("set_volume", { volume: "10" }), /Premium/);
});

test("playback moves to another device, and starts a playlist", async () => {
  const server = await spotify();
  const host = await signedIn(server);
  const devices = await host.choices("transfer_playback");
  assert.equal(devices.arguments.device?.[1]?.label, "Kitchen (speaker)");
  await host.invoke("transfer_playback", { device: "d2" });
  assert.deepEqual(server.transferred, ["d2"]);
  await host.invoke("play_playlist", { playlist: "pl1" });
  assert.deepEqual(server.played.at(-1), { context_uri: "spotify:playlist:pl1" });
});
