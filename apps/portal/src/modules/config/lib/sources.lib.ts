import type { DataSource } from "../../../services/api/api.ts";

/**
 * Data sources, as the editor handles them: ids from names, starting points,
 * and the paths a click on a JSON answer turns into.
 */

/** An id from a name: "Bitcoin price" → "bitcoin-price", unique among `taken`. */
export function idFor(name: string, taken: readonly string[]): string {
  const base =
    name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .replace(/^[^a-z]+/, "")
      .slice(0, 28) || "source";
  let id = base;
  for (let n = 2; taken.includes(id); n += 1) id = `${base}-${n}`;
  return id;
}

/** A field name from a JSON path's last part: `json.current.temperature_2m` → `temperature_2m`. */
export function fieldNameFor(path: string, taken: readonly string[]): string {
  const last = path.split(".").filter((part) => part !== "*" && !/^\d+$/.test(part)).pop() ?? "value";
  let base = last.replace(/[^A-Za-z0-9_]/g, "_").slice(0, 28) || "value";
  if (["ok", "error", "updated"].includes(base)) base = `${base}_value`;
  let name = base;
  for (let n = 2; taken.includes(name); n += 1) name = `${base}_${n}`;
  return name;
}

/** The expression that reads `path` in an answer: `json.a.b.0`. */
export function pathExpression(path: (string | number)[]): string {
  return ["json", ...path.map(String)].join(".");
}

/** Whether a JSON path reads a value a field can hold, or a list a graph can draw. */
export function describeJson(value: unknown): "number" | "text" | "bool" | "numbers" | "list" | "object" | "null" {
  if (value === null || value === undefined) return "null";
  if (typeof value === "number") return "number";
  if (typeof value === "string") return "text";
  if (typeof value === "boolean") return "bool";
  if (Array.isArray(value)) return value.every((item) => typeof item === "number") && value.length > 0 ? "numbers" : "list";
  return "object";
}

/** Starting points: a source that works as soon as it is added. */
export const STARTERS: readonly { label: string; hint: string; source: Omit<DataSource, "id"> }[] = [
  {
    label: "Weather",
    hint: "Open-Meteo: temperature and sky for a place — no key needed",
    source: {
      name: "Weather",
      kind: "http",
      everySeconds: 900,
      args: [
        { name: "lat", default: "52.52", description: "Latitude" },
        { name: "lon", default: "13.41", description: "Longitude" },
      ],
      http: {
        url: "https://api.open-meteo.com/v1/forecast?latitude={{args.lat}}&longitude={{args.lon}}&current=temperature_2m,weather_code,wind_speed_10m&hourly=temperature_2m&forecast_days=1",
      },
      fields: [
        { name: "temp", value: "json.current.temperature_2m" },
        {
          name: "sky",
          value:
            'json.current.weather_code | map(0: "Clear", 1: "Mostly clear", 2: "Partly cloudy", 3: "Cloudy", 45: "Fog", 48: "Fog", 51: "Drizzle", 61: "Rain", 63: "Rain", 65: "Heavy rain", 71: "Snow", 80: "Showers", 95: "Thunder", _: "Mixed")',
        },
        {
          name: "icon",
          value: 'json.current.weather_code | map(0: "sun", 1: "sun", 2: "cloud", 3: "cloud", 61: "cloud-rain", 63: "cloud-rain", 65: "cloud-rain", 80: "cloud-rain", 95: "bolt", _: "cloud")',
        },
        { name: "wind", value: "json.current.wind_speed_10m" },
        { name: "today", value: "json.hourly.temperature_2m", series: true },
      ],
    },
  },
  {
    label: "Crypto price",
    hint: "CoinGecko: a coin's price and its change over a day",
    source: {
      name: "Crypto",
      kind: "http",
      everySeconds: 120,
      args: [
        { name: "coin", default: "bitcoin", description: "CoinGecko's id for the coin" },
        { name: "in", default: "usd", description: "The currency, lowercase" },
      ],
      http: { url: "https://api.coingecko.com/api/v3/simple/price?ids={{args.coin}}&vs_currencies={{args.in}}&include_24hr_change=true" },
      fields: [
        { name: "price", value: "json.{{args.coin}}.{{args.in}}" },
        { name: "change", value: "json.{{args.coin}}.{{args.in}}_24h_change" },
      ],
    },
  },
  {
    label: "Exchange rate",
    hint: "Frankfurter: one currency in another, from the ECB",
    source: {
      name: "Exchange rate",
      kind: "http",
      everySeconds: 3600,
      args: [
        { name: "from", default: "EUR", description: "From" },
        { name: "to", default: "USD", description: "To" },
      ],
      http: { url: "https://api.frankfurter.app/latest?from={{args.from}}&to={{args.to}}" },
      fields: [{ name: "rate", value: "json.rates.{{args.to}}" }],
    },
  },
  {
    label: "Calendar",
    hint: "Your calendar's next event, from its secret iCal address (Google, Outlook, iCloud)",
    source: {
      name: "Calendar",
      kind: "http",
      everySeconds: 300,
      args: [{ name: "url", default: "https://", description: "The calendar's iCal (.ics) address" }],
      http: { url: "{{args.url}}" },
      fields: [
        { name: "next", value: "json.events.0.summary" },
        { name: "at", value: "json.events.0.start" },
        { name: "soon", value: "json.events.0.start - now" },
        { name: "where", value: "json.events.0.location" },
      ],
    },
  },
  {
    label: "Any web address",
    hint: "Fetch a URL that answers JSON or text, and pick what to read",
    source: { name: "Web", kind: "http", http: { url: "https://" }, fields: [] },
  },
  {
    label: "A program's output",
    hint: "Run a program every so often and read what it prints",
    source: { name: "Program", kind: "command", everySeconds: 60, command: { program: "", args: [] }, fields: [{ name: "out", value: "text" }] },
  },
];

/**
 * Which kept secret a key would use for a source's argument: its own, the
 * source's (the first one any key gave, or the Data tab's), or none. `secrets`
 * is the agent's list of `source/widget/arg`.
 */
export function keptSecret(secrets: readonly string[], source: string, widget: string | null, arg: string): "own" | "shared" | null {
  if (widget && secrets.includes(`${source}/${widget}/${arg}`)) return "own";
  if (secrets.includes(`${source}//${arg}`)) return "shared";
  return null;
}
