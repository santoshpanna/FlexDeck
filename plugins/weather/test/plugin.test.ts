import assert from "node:assert/strict";
import { test } from "node:test";

import { checkDefinition, harness } from "@flexdeck/plugin-sdk/testing";

import plugin, { describe, http } from "../src/index.ts";

/** Open-Meteo's answer shape, without the network. */
http.get = async (url: string) => {
  const lat = Number(new URL(url).searchParams.get("latitude"));
  return {
    current: { temperature_2m: lat > 40 ? 18.4 : 29.1, weather_code: lat > 40 ? 61 : 0, wind_speed_10m: 12 },
    hourly: { temperature_2m: [14, 15, 17, 18.4] },
  };
};

test("the definition is one the agent will accept", async () => {
  await checkDefinition(plugin, new URL("../plugin.json", import.meta.url));
  const manifest = plugin.manifest(4);
  assert.deepEqual(manifest.uses, ["series", "presets", "feeds"]);
  assert.equal(manifest.presets?.[0]?.widget.sources instanceof Array, true);
});

test("each key gets its own place's weather, and today's temperatures", async () => {
  const host = await harness(plugin);
  await host.watch("w1", "place", { lat: "52.52", lon: "13.41" });
  await host.watch("w2", "place", { lat: "1.35", lon: "103.8" });

  assert.deepEqual(host.feed("w1"), { temp: 18.4, wind: 12, sky: "Rain", icon: "cloud-rain" });
  assert.deepEqual(host.feed("w2"), { temp: 29.1, wind: 12, sky: "Clear", icon: "sun" });
  assert.deepEqual(host.series("weather.@w1.today"), [14, 15, 17, 18.4]);

  await host.unwatch("w1");
  await host.unwatch("w2");
});

test("a key without a place gets the example's", async () => {
  const host = await harness(plugin);
  await host.watch("w3", "place");
  assert.equal(host.feed("w3").temp, 18.4, "Berlin, the example");
  await host.unwatch("w3");
});

test("weather codes read as words and icons", () => {
  assert.deepEqual(describe(0), { sky: "Clear", icon: "sun" });
  assert.deepEqual(describe(3), { sky: "Cloudy", icon: "cloud" });
  assert.deepEqual(describe(73), { sky: "Snow", icon: "cloud-snow" });
  assert.deepEqual(describe(96), { sky: "Thunder", icon: "bolt" });
});
