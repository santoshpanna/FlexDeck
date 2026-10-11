import type { WireConfig, WireFlow, WireWidget } from "../../../services/api/api.ts";
import { gridSpecOf, profileOf } from "./config.lib.ts";
import { findPage, firstFreeCell, nextWidgetId, spansOf, withPage } from "./editing.lib.ts";
import { CONFIG_SCHEMA } from "../../../services/api/api.ts";

/**
 * Ready-made keys: a face, what it reads, and what a press does — dropped on
 * the deck in one click. The gallery is where most people start, so every one
 * here works as soon as it lands (or says which plugin or source it needs).
 */

export type PresetCategory = "System" | "Time" | "Media" | "Web" | "Controls" | "Plugins";

export interface Preset {
  id: string;
  name: string;
  category: PresetCategory;
  description: string;
  w: number;
  h: number;
  widget: Omit<WireWidget, "id" | "col" | "row" | "w" | "h">;
  /** A data source it reads, added from the starters (by label) when missing. */
  needs?: { starter: string; id: string };
}

const press = (plugin: string, action: string, args: Record<string, string> = {}): WireFlow => ({
  trigger: { gesture: "GESTURE_SHORT_PRESS" },
  blocks: [{ block: "doAction", plugin, action, args: Object.fromEntries(Object.entries(args).map(([k, v]) => [k, { text: v }])) }],
});

const HOT = "#7f1d1d";
const GOOD = "#4ade80";
const BAD = "#ef4444";

export const PRESETS: readonly Preset[] = [
  // ---- System -----------------------------------------------------------------
  {
    id: "cpu-gauge",
    name: "CPU gauge",
    category: "System",
    description: "How busy the processor is, as a ring that turns red past 85%.",
    w: 1,
    h: 1,
    widget: {
      face: {
        layout: "FACE_LAYOUT_DIAL",
        elements: [
          { slot: "SLOT_VISUAL", kind: "ELEMENT_KIND_GAUGE", template: "{{system.cpu.usage}}", rules: [{ when: "system.cpu.usage > 85", color: BAD }] },
          { slot: "SLOT_VALUE", template: "{{ system.cpu.usage | round }}%" },
          { slot: "SLOT_CAPTION", template: "CPU" },
        ],
      },
    },
  },
  {
    id: "cpu-graph",
    name: "CPU history",
    category: "System",
    description: "The last minute of processor load behind the number.",
    w: 2,
    h: 1,
    widget: {
      face: {
        layout: "FACE_LAYOUT_VALUE",
        background: { graph: "system.cpu.usage" },
        rules: [{ when: "system.cpu.usage > 90", color: HOT }],
        elements: [
          { slot: "SLOT_TOP", template: "CPU" },
          { slot: "SLOT_VALUE", template: "{{ system.cpu.usage | round }}%", text: { font: "FONT_MONO" } },
        ],
      },
    },
  },
  {
    id: "memory",
    name: "Memory",
    category: "System",
    description: "Memory in use, as a bar, with how many gigabytes.",
    w: 1,
    h: 1,
    widget: {
      face: {
        layout: "FACE_LAYOUT_VALUE",
        elements: [
          { slot: "SLOT_TOP", template: "RAM" },
          { slot: "SLOT_VALUE", template: "{{ sensors.memory.used | round }}%" },
          { slot: "SLOT_CAPTION", template: "{{ sensors.memory.used_gb | round(1) }} / {{ sensors.memory.total_gb | round }} GB" },
          { slot: "SLOT_VISUAL", kind: "ELEMENT_KIND_BAR", template: "{{sensors.memory.used}}", rules: [{ when: "sensors.memory.used > 90", color: BAD }] },
        ],
      },
    },
  },
  {
    id: "gpu",
    name: "GPU gauge",
    category: "System",
    description: "How busy the graphics card is.",
    w: 1,
    h: 1,
    widget: {
      face: {
        layout: "FACE_LAYOUT_DIAL",
        elements: [
          { slot: "SLOT_VISUAL", kind: "ELEMENT_KIND_GAUGE", template: "{{sensors.gpu.usage}}", color: "#a78bfa" },
          { slot: "SLOT_VALUE", template: "{{ sensors.gpu.usage | round }}%" },
          { slot: "SLOT_CAPTION", template: "GPU" },
        ],
      },
    },
  },
  {
    id: "network",
    name: "Network",
    category: "System",
    description: "Download and upload speed, the download drawn behind.",
    w: 2,
    h: 1,
    widget: {
      face: {
        layout: "FACE_LAYOUT_VALUE",
        background: { graph: "sensors.network.down", graphColor: "#38bdf8" },
        elements: [
          { slot: "SLOT_TOP", template: "↓ Network" },
          { slot: "SLOT_VALUE", template: "{{ sensors.network.down | bytes }}/s", text: { font: "FONT_MONO" } },
          { slot: "SLOT_CAPTION", template: "↑ {{ sensors.network.up | bytes }}/s" },
        ],
      },
    },
  },
  {
    id: "disk",
    name: "Disk space",
    category: "System",
    description: "How full the system drive is.",
    w: 1,
    h: 1,
    widget: {
      face: {
        layout: "FACE_LAYOUT_VALUE",
        elements: [
          { slot: "SLOT_TOP", template: "C:" },
          { slot: "SLOT_VALUE", template: "{{ sensors.disk.free_gb | round }} GB" },
          { slot: "SLOT_CAPTION", template: "free" },
          { slot: "SLOT_VISUAL", kind: "ELEMENT_KIND_BAR", template: "{{sensors.disk.used}}" },
        ],
      },
    },
  },
  {
    id: "battery",
    name: "Battery",
    category: "System",
    description: "Charge left, red when low, and whether it is charging.",
    w: 1,
    h: 1,
    widget: {
      face: {
        layout: "FACE_LAYOUT_DIAL",
        elements: [
          { slot: "SLOT_VISUAL", kind: "ELEMENT_KIND_GAUGE", template: "{{sensors.battery.level}}", color: GOOD, rules: [{ when: "sensors.battery.level < 20", color: BAD, pulse: true }] },
          { slot: "SLOT_VALUE", template: "{{ sensors.battery.level | round }}%" },
          { slot: "SLOT_CAPTION", template: "{{ if(sensors.battery.charging, \"Charging\", \"Battery\") }}" },
        ],
      },
    },
  },

  // ---- Time -------------------------------------------------------------------
  {
    id: "clock",
    name: "Digital clock",
    category: "Time",
    description: "The time, and the day under it.",
    w: 2,
    h: 1,
    widget: {
      face: {
        layout: "FACE_LAYOUT_VALUE",
        elements: [
          { slot: "SLOT_VALUE", kind: "ELEMENT_KIND_CLOCK", clock: { format: "%H:%M" } },
          { slot: "SLOT_CAPTION", template: "{{time.weekday}}, {{time.month}} {{time.day}}" },
        ],
      },
    },
  },
  {
    id: "analog",
    name: "Analog clock",
    category: "Time",
    description: "A clock face with a sweeping second hand.",
    w: 1,
    h: 1,
    widget: {
      face: {
        layout: "FACE_LAYOUT_DIAL",
        elements: [
          { slot: "SLOT_VISUAL", kind: "ELEMENT_KIND_CLOCK", clock: { analog: true, seconds: true } },
          { slot: "SLOT_CAPTION", template: "{{time.weekday}}" },
        ],
      },
    },
  },
  {
    id: "world-clock",
    name: "World clock",
    category: "Time",
    description: "The time somewhere else. Change the city in its clock.",
    w: 1,
    h: 1,
    widget: {
      face: {
        layout: "FACE_LAYOUT_VALUE",
        elements: [
          { slot: "SLOT_TOP", template: "Tokyo" },
          { slot: "SLOT_VALUE", kind: "ELEMENT_KIND_CLOCK", clock: { format: "%H:%M", zone: "Asia/Tokyo" } },
          { slot: "SLOT_CAPTION", kind: "ELEMENT_KIND_CLOCK", clock: { format: "%a", zone: "Asia/Tokyo" } },
        ],
      },
    },
  },

  // ---- Timers -----------------------------------------------------------------
  {
    id: "pomodoro",
    name: "Pomodoro",
    category: "Time",
    description: "25 minutes of focus: tap to start or pause, hold to reset. Pulses when the time is up.",
    w: 1,
    h: 1,
    widget: {
      flows: [
        press("timer", "toggle", { name: "focus", minutes: "25" }),
        { ...press("timer", "reset", { name: "focus" }), trigger: { gesture: "GESTURE_LONG_PRESS" } },
      ],
      face: {
        layout: "FACE_LAYOUT_VALUE",
        rules: [
          { when: "timer.focus.done", color: "#7f1d1d", pulse: true },
          { when: "timer.focus.running", color: "#14532d" },
        ],
        elements: [
          { slot: "SLOT_TOP", template: "Focus" },
          { slot: "SLOT_VALUE", kind: "ELEMENT_KIND_TIMER", variable: "focus" },
          { slot: "SLOT_CAPTION", template: "{{ if(timer.focus.running, \"tap to pause\", if(timer.focus.done, \"time for a break\", \"tap to start\")) }}" },
        ],
      },
    },
  },
  {
    id: "tea-timer",
    name: "Tea timer",
    category: "Time",
    description: "A tea timer — four minutes, or change it under Does. Tap to start or pause, hold to reset.",
    w: 1,
    h: 1,
    widget: {
      flows: [
        press("timer", "toggle", { name: "tea", minutes: "4" }),
        { ...press("timer", "reset", { name: "tea" }), trigger: { gesture: "GESTURE_LONG_PRESS" } },
      ],
      face: {
        layout: "FACE_LAYOUT_VALUE",
        rules: [{ when: "timer.tea.done", color: "#7f1d1d", pulse: true }],
        elements: [
          { slot: "SLOT_TOP", template: "Tea" },
          { slot: "SLOT_VALUE", kind: "ELEMENT_KIND_TIMER", variable: "tea" },
          { slot: "SLOT_CAPTION", template: "{{ if(timer.tea.done, \"ready\", \"\") }}" },
        ],
      },
    },
  },

  // ---- Media ------------------------------------------------------------------
  {
    id: "now-playing",
    name: "Now playing",
    category: "Media",
    description: "Album art, the song and the artist, and how far through. Press to play or pause.",
    w: 2,
    h: 2,
    widget: {
      flows: [press("media", "play_pause")],
      face: {
        layout: "FACE_LAYOUT_ART",
        background: { image: "{{media.current.art}}", color: "#1f1f23" },
        elements: [
          { slot: "SLOT_TITLE", template: "{{ media.current.title | default(\"Nothing playing\") }}", text: { marquee: true } },
          { slot: "SLOT_CAPTION", template: "{{ media.current.artist | default(\"\") }}" },
          { slot: "SLOT_VISUAL", kind: "ELEMENT_KIND_BAR", template: "{{media.current.progress}}", color: "#ffffff" },
          { slot: "SLOT_BADGE", template: "{{ if(media.current.playing, \"\", \"❚❚\") }}" },
        ],
      },
    },
  },
  {
    id: "now-playing-controls",
    name: "Now playing + controls",
    category: "Media",
    description: "Art, song and artist, with previous, play / pause and next along the bottom — one wide key.",
    w: 3,
    h: 1,
    widget: {
      flows: [
        { ...press("media", "previous"), zone: "prev" },
        { ...press("media", "play_pause"), zone: "play" },
        { ...press("media", "next"), zone: "next" },
      ],
      face: {
        layout: "FACE_LAYOUT_SPLIT",
        background: { color: "#1f1f23" },
        elements: [
          { slot: "SLOT_VISUAL", kind: "ELEMENT_KIND_ICON", template: "{{ media.current.art | default(\"music\") }}" },
          { slot: "SLOT_TITLE", template: "{{ media.current.title | default(\"Nothing playing\") }}", text: { marquee: true } },
          { slot: "SLOT_CAPTION", template: "{{ media.current.artist | default(\"\") }}" },
        ],
        zones: [
          { name: "prev", icon: "skip-back" },
          { name: "play", icon: "play", when: "media.current.playing", iconOn: "pause" },
          { name: "next", icon: "skip-forward" },
        ],
      },
    },
  },
  ...(
    [
      ["previous", "Previous", "Back a track", "skip-back"],
      ["play_pause", "Play / pause", "Play or pause whatever is playing", "play"],
      ["next", "Next", "On a track", "skip-forward"],
    ] as const
  ).map(
    ([action, name, description, icon]): Preset => ({
      id: `media-${action}`,
      name,
      category: "Media",
      description,
      w: 1,
      h: 1,
      widget: { flows: [press("media", action)], props: { label: name, icon } },
    }),
  ),
  {
    id: "mic",
    name: "Microphone mute",
    category: "Controls",
    description: "Mutes the microphone; red while muted.",
    w: 1,
    h: 1,
    widget: {
      props: { label: "Mic", icon: "mic" },
      flows: [press("system", "toggle_input_mute")],
      states: {
        looks: [{ id: "live" }, { id: "muted", label: "Muted", icon: "mic-off", bgColor: "#7f1d1d" }],
        variable: { variable: "system.audio.input.muted", cases: [{ value: "true", state: "muted" }], otherwise: "live" },
      },
    },
  },

  // ---- Controls ---------------------------------------------------------------
  {
    id: "next-page",
    name: "Next page",
    category: "Controls",
    description: "Turns the deck to its next page.",
    w: 1,
    h: 1,
    widget: { props: { label: "Next page", icon: "arrow-right" }, flows: [press("deck", "next_page")] },
  },
  {
    id: "counter",
    name: "Counter",
    category: "Controls",
    description: "Counts presses; a long press starts again. A value of your own: vars.count.",
    w: 1,
    h: 1,
    widget: {
      flows: [
        press("vars", "add", { name: "count", by: "1" }),
        { trigger: { gesture: "GESTURE_LONG_PRESS" }, blocks: [{ block: "doAction", plugin: "vars", action: "set", args: { name: { text: "count" }, value: { text: "0" } } }] },
      ],
      face: {
        layout: "FACE_LAYOUT_VALUE",
        elements: [
          { slot: "SLOT_TOP", template: "Count" },
          { slot: "SLOT_VALUE", template: "{{ vars.count | default(0) }}", text: { font: "FONT_MONO" } },
          { slot: "SLOT_CAPTION", template: "hold to reset" },
        ],
      },
    },
  },

  // ---- Web --------------------------------------------------------------------
  {
    id: "weather",
    name: "Weather",
    category: "Web",
    description: "Temperature and sky now, today's temperatures behind. Set its place in Its own data.",
    w: 2,
    h: 1,
    needs: { starter: "Weather", id: "weather" },
    widget: {
      sources: [{ source: "weather" }],
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
  {
    id: "crypto",
    name: "Crypto ticker",
    category: "Web",
    description: "A coin's price, green or red with its change over a day. Set the coin in Its own data.",
    w: 2,
    h: 1,
    needs: { starter: "Crypto price", id: "crypto" },
    widget: {
      sources: [{ source: "crypto" }],
      face: {
        layout: "FACE_LAYOUT_VALUE",
        rules: [{ when: "this.change < -5", color: HOT, pulse: true }],
        elements: [
          { slot: "SLOT_TOP", template: "Bitcoin" },
          { slot: "SLOT_VALUE", template: "{{ this.price | currency }}", text: { font: "FONT_MONO" } },
          {
            slot: "SLOT_CAPTION",
            template: "{{ this.change | round(2) | sign }}% today",
            rules: [
              { when: "this.change < 0", color: BAD },
              { when: "this.change >= 0", color: GOOD },
            ],
          },
        ],
      },
    },
  },
  {
    id: "calendar",
    name: "Next meeting",
    category: "Web",
    description: "Your calendar's next event and how soon it starts. Paste your calendar's iCal address into its source.",
    w: 2,
    h: 1,
    needs: { starter: "Calendar", id: "calendar" },
    widget: {
      face: {
        layout: "FACE_LAYOUT_VALUE",
        rules: [{ when: "src.calendar.soon <= 300 and src.calendar.soon > 0", color: HOT, pulse: true }],
        elements: [
          { slot: "SLOT_TOP", template: "{{ src.calendar.at | time(\"%a %H:%M\") }}" },
          { slot: "SLOT_VALUE", template: "{{ src.calendar.next | default(\"Nothing coming\") | truncate(28) }}", text: { size: "TEXT_SIZE_L", bold: true, maxLines: 2 } },
          { slot: "SLOT_CAPTION", template: "{{ if(src.calendar.soon > 0, \"in \" + (src.calendar.soon | duration), \"now\") }}" },
        ],
      },
    },
  },
];

/** The built-in categories; "Plugins" shows when a plugin brings presets. */
export const PRESET_CATEGORIES: readonly PresetCategory[] = ["System", "Time", "Media", "Web", "Controls"];

/**
 * Put a preset on a page at its first free place, at its size if it fits,
 * else as small as it can go. `null` when the page has no room at all.
 */
export function placePreset(config: WireConfig, pageId: string, preset: Preset): { config: WireConfig; widgetId: string } | null {
  const profile = profileOf(config);
  const page = findPage(config, pageId);
  if (!profile || !page) return null;
  const spec = gridSpecOf(profile);
  const spans = spansOf(page);
  const sizes = [
    [preset.w, preset.h],
    [preset.w, 1],
    [1, 1],
  ] as const;
  for (const [w, h] of sizes) {
    const cell = firstFreeCell(spec, spans, w, h);
    if (!cell) continue;
    const id = nextWidgetId(spans.map((span) => span.id));
    const widget: WireWidget = { ...structuredClone(preset.widget), id, type: "WIDGET_TYPE_BUTTON", col: cell.col, row: cell.row, w, h };
    if (!widget.props) widget.props = { label: preset.name };
    else if (!widget.props.label) widget.props = { ...widget.props, label: preset.name };
    const next = withPage(config, pageId, (target) => ({ ...target, widgets: [...(target.widgets ?? []), widget] }));
    return { config: (next.schema ?? 0) >= CONFIG_SCHEMA ? next : { ...next, schema: CONFIG_SCHEMA }, widgetId: id };
  }
  return null;
}

/** Whole pages to start from: a few presets, placed in order. */
export const STARTER_LAYOUTS: readonly { id: string; name: string; description: string; presets: readonly string[] }[] = [
  {
    id: "dashboard",
    name: "Desk dashboard",
    description: "The time, the weather, and how the machine is doing.",
    presets: ["clock", "weather", "cpu-gauge", "memory", "gpu", "network", "disk"],
  },
  {
    id: "media",
    name: "Media",
    description: "What is playing, and the keys to control it.",
    presets: ["now-playing", "media-previous", "media-play_pause", "media-next", "mic"],
  },
  {
    id: "focus",
    name: "Focus",
    description: "A clock, your next meeting, a counter and the mic.",
    presets: ["pomodoro", "analog", "calendar", "counter", "mic", "world-clock"],
  },
];

/** Place every preset of a layout on a page, as many as fit. */
export function placeLayout(config: WireConfig, pageId: string, layoutId: string): { config: WireConfig; placed: number; needs: Preset["needs"][] } {
  const layout = STARTER_LAYOUTS.find((candidate) => candidate.id === layoutId);
  let current = config;
  let placed = 0;
  const needs: Preset["needs"][] = [];
  for (const presetId of layout?.presets ?? []) {
    const preset = PRESETS.find((candidate) => candidate.id === presetId);
    if (!preset) continue;
    const result = placePreset(current, pageId, preset);
    if (!result) break;
    current = result.config;
    placed += 1;
    if (preset.needs) needs.push(preset.needs);
  }
  return { config: current, placed, needs };
}
