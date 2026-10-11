/**
 * Weather, as a plugin: the reference for plugin ABI 4.
 *
 * The same thing can be made without code — the "Weather" data source in the
 * editor's Data tab, or the weather recipe. This one shows what a plugin adds:
 *
 * - a **feed** (`place`): each key that uses it gets its own place, and its
 *   own values, read as `{{this.temp}}`;
 * - a **series**: today's temperatures, for a graph behind the number;
 * - a **preset**: a ready-made key in the editor's gallery.
 *
 * It uses Open-Meteo, which needs no key.
 */

import { definePlugin, type Context } from "@flexdeck/plugin-sdk";

/** A WMO weather code as words and a bundled icon. */
export function describe(code: number): { sky: string; icon: string } {
  if (code === 0) return { sky: "Clear", icon: "sun" };
  if (code <= 2) return { sky: "Partly cloudy", icon: "cloud" };
  if (code === 3) return { sky: "Cloudy", icon: "cloud" };
  if (code <= 48) return { sky: "Fog", icon: "cloud" };
  if (code <= 67 || (code >= 80 && code <= 82)) return { sky: "Rain", icon: "cloud-rain" };
  if (code <= 77 || code === 85 || code === 86) return { sky: "Snow", icon: "cloud-snow" };
  if (code >= 95) return { sky: "Thunder", icon: "bolt" };
  return { sky: "Mixed", icon: "cloud" };
}

/** How it fetches; a test replaces it. */
export const http = {
  get: async (url: string): Promise<unknown> => {
    const response = await fetch(url, { signal: AbortSignal.timeout(10_000) });
    if (!response.ok) throw new Error(`Open-Meteo answered ${response.status}`);
    return response.json();
  },
};

/** Each watching key: its place and its timer. */
const places = new Map<string, { lat: string; lon: string; timer: ReturnType<typeof setInterval> }>();

/** Fetch one key's place and publish its values. */
async function refresh(instance: string, ctx: Context): Promise<void> {
  const place = places.get(instance);
  if (!place) return;
  const url =
    `https://api.open-meteo.com/v1/forecast?latitude=${encodeURIComponent(place.lat)}&longitude=${encodeURIComponent(place.lon)}` +
    "&current=temperature_2m,weather_code,wind_speed_10m&hourly=temperature_2m&forecast_days=1";
  try {
    const answer = (await http.get(url)) as {
      current: { temperature_2m: number; weather_code: number; wind_speed_10m: number };
      hourly?: { temperature_2m?: number[] };
    };
    const { sky, icon } = describe(answer.current.weather_code);
    ctx.feed(instance).set({
      temp: answer.current.temperature_2m,
      wind: answer.current.wind_speed_10m,
      sky,
      icon,
    });
    ctx.series.set(`weather.@${instance}.today`, answer.hourly?.temperature_2m ?? []);
  } catch (error) {
    ctx.log.warn(`${instance}: ${error instanceof Error ? error.message : String(error)}`);
  }
}

export default definePlugin({
  id: "weather",
  name: "Weather",
  version: "0.1.0",
  description: "The weather where each key says, from Open-Meteo. No account needed.",
  author: "flexdeck",
  permissions: ["network"],
  settings: {
    minutes: { kind: "number", description: "How often to look again, in minutes", default: "15" },
  },
  actions: {
    refresh: {
      description: "Look again now, for every key",
      run: async (_args, ctx) => {
        await Promise.all([...places.keys()].map((instance) => refresh(instance, ctx)));
      },
    },
  },
  feeds: {
    place: {
      description: "The weather at a place",
      arguments: {
        lat: { kind: "text", description: "Latitude", example: "52.52" },
        lon: { kind: "text", description: "Longitude", example: "13.41" },
      },
      fields: {
        temp: { kind: "number", description: "Temperature, °C" },
        wind: { kind: "number", description: "Wind, km/h" },
        sky: { kind: "text", description: "The sky, in words" },
        icon: { kind: "text", description: "An icon for the sky" },
      },
      watch: async (instance, { lat, lon }, ctx) => {
        const old = places.get(instance);
        if (old) clearInterval(old.timer);
        const minutes = Math.max(5, Number(ctx.settings.minutes) || 15);
        const timer = setInterval(() => void refresh(instance, ctx), minutes * 60_000);
        timer.unref?.();
        places.set(instance, { lat, lon, timer });
        await refresh(instance, ctx);
      },
      unwatch: (instance) => {
        const place = places.get(instance);
        if (place) clearInterval(place.timer);
        places.delete(instance);
      },
    },
  },
  presets: [
    {
      id: "weather",
      name: "Weather",
      category: "Web",
      description: "The weather now, today's temperatures behind it. Set the place in Its own data.",
      w: 2,
      h: 1,
      widget: {
        sources: [{ source: "weather/place" }],
        face: {
          layout: "FACE_LAYOUT_SPLIT",
          background: { graph: "this.today", graphColor: "#38bdf8" },
          elements: [
            { slot: "SLOT_VISUAL", kind: "ELEMENT_KIND_ICON", template: "{{this.icon}}" },
            { slot: "SLOT_TITLE", template: "{{ this.temp | temp }}", text: { size: "TEXT_SIZE_XL", bold: true } },
            { slot: "SLOT_CAPTION", template: "{{this.sky}} · {{ this.wind | round }} km/h" },
          ],
        },
      },
    },
  ],
  stop: () => {
    for (const place of places.values()) clearInterval(place.timer);
    places.clear();
  },
});
