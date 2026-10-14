import type { Block, Gesture, WireConfig, WireFlow, WireWidget } from "../../../services/api/api.ts";
import { gridSpecOf, profileOf, withSchemaFor } from "./config.lib.ts";
import { findPage, findWidget, firstFreeCell, nextWidgetId, widgetIdsOf, resizeWidget, spansOf, withPage, withWidget } from "./editing.lib.ts";

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
    description: "Temperature and sky now, today's temperatures behind. Set its place in its source.",
    w: 2,
    h: 1,
    needs: { starter: "Weather", id: "weather" },
    widget: {
      face: {
        layout: "FACE_LAYOUT_SPLIT",
        background: { graph: "src.weather.today", graphColor: "#38bdf8" },
        elements: [
          { slot: "SLOT_VISUAL", kind: "ELEMENT_KIND_ICON", template: "{{src.weather.icon}}" },
          { slot: "SLOT_TITLE", template: "{{ src.weather.temp | temp }}", text: { size: "TEXT_SIZE_XL", bold: true } },
          { slot: "SLOT_CAPTION", template: "{{src.weather.sky}} · {{ src.weather.wind | round }} km/h" },
        ],
      },
    },
  },
  {
    id: "crypto",
    name: "Crypto ticker",
    category: "Web",
    description: "A coin's price, green or red with its change over a day. Set the coin in its source.",
    w: 2,
    h: 1,
    needs: { starter: "Crypto price", id: "crypto" },
    widget: {
      face: {
        layout: "FACE_LAYOUT_VALUE",
        rules: [{ when: "src.crypto.change < -5", color: HOT, pulse: true }],
        elements: [
          { slot: "SLOT_TOP", template: "Bitcoin" },
          { slot: "SLOT_VALUE", template: "{{ src.crypto.price | currency }}", text: { font: "FONT_MONO" } },
          {
            slot: "SLOT_CAPTION",
            template: "{{ src.crypto.change | round(2) | sign }}% today",
            rules: [
              { when: "src.crypto.change < 0", color: BAD },
              { when: "src.crypto.change >= 0", color: GOOD },
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
    const id = nextWidgetId(widgetIdsOf(config));
    const widget: WireWidget = { ...structuredClone(preset.widget), id, type: "WIDGET_TYPE_BUTTON", col: cell.col, row: cell.row, w, h };
    if (!widget.props) widget.props = { label: preset.name };
    else if (!widget.props.label) widget.props = { ...widget.props, label: preset.name };
    const next = withPage(config, pageId, (target) => ({ ...target, widgets: [...(target.widgets ?? []), widget] }));
    return { config: withSchemaFor(next), widgetId: id };
  }
  return null;
}


// ---- A preset's halves, and its look on a key you already have ----------------

/** What a preset brings: a face that follows live values, and the gestures it answers. */
export function presetHalves(preset: Preset): { live: boolean; gestures: Gesture[] } {
  const { face, props, states, flows } = preset.widget;
  const live =
    (face?.elements ?? []).some((element) => element.template?.includes("{{") || element.kind === "ELEMENT_KIND_CLOCK" || element.kind === "ELEMENT_KIND_TIMER") ||
    Boolean(face?.background?.graph || face?.background?.image?.includes("{{") || face?.rules?.length) ||
    Boolean(props?.labelTemplate || states?.variable);
  const gestures = [...new Set((flows ?? []).map((flow) => flow.trigger?.gesture ?? "GESTURE_SHORT_PRESS"))];
  return { live, gestures };
}

/**
 * Whether a preset has a look that can go on another key: a face without its
 * own strip of buttons (those belong to its flows), or just an icon. A look
 * made of states would replace the key's own — its scene light — so not those.
 */
export function isLookPreset(preset: Preset): boolean {
  const { face, props, states } = preset.widget;
  if (states) return false;
  if (face) return !face.zones?.length;
  return Boolean(props?.icon);
}

/**
 * Give a key a preset's look, keeping everything it does: its flows, its
 * states (the scene light), its label, its hold. A face brings the data it
 * reads (`this.*`) with it; a strip of buttons the key already had stays, as
 * its flows still answer to it. A slider has no face to give.
 */
export function applyPresetLook(config: WireConfig, widgetId: string, preset: Preset): WireConfig {
  const target = findWidget(config, widgetId)?.widget;
  if (!isLookPreset(preset) || !target || target.type === "WIDGET_TYPE_SLIDER") return config;
  const next = withWidget(config, widgetId, (widget) => {
    const { face, props, sources } = sharedReads(preset.widget);
    if (!face) {
      return { ...widget, props: { ...(widget.props ?? {}), icon: props!.icon! } };
    }
    const look: WireWidget = { ...widget, face };
    if (widget.face?.zones?.length) look.face = { ...look.face, zones: widget.face.zones };
    if (sources?.length) look.sources = sources;
    return look;
  });
  if (next === config) return config;
  return withSchemaFor(next);
}

/**
 * A preset's key without a data source run of its own: a plugin's preset that
 * still asks for one reads the shared source instead, `this.temp` becoming
 * `src.weather.temp`. Its own settings for the run are dropped; the source's
 * are used. A plugin's feed (`plugin/feed`) is per key by nature and stays.
 * The agent's `upgrade::shared_sources` does the same to decks already saved.
 */
export function sharedReads<W extends Pick<WireWidget, "sources">>(widget: W): W {
  const copy = structuredClone(widget);
  const source = copy.sources?.[0]?.source;
  if (!source || source.includes("/")) return copy;
  const feeds = (copy.sources ?? []).filter((used) => used.source?.includes("/"));
  delete copy.sources;
  const shared = JSON.parse(JSON.stringify(copy).replace(/(?<![\w.])this\.(?=[A-Za-z_])/g, `src.${source}.`)) as W;
  if (feeds.length) shared.sources = feeds;
  return shared;
}

/** Whether the key can grow to the preset's own size where it stands. */
export function presetFits(config: WireConfig, widgetId: string, preset: Preset): boolean {
  const widget = findWidget(config, widgetId)?.widget;
  if (!widget || ((widget.w ?? 1) === preset.w && (widget.h ?? 1) === preset.h)) return false;
  return resizeWidget(config, widgetId, preset.w, preset.h) !== config;
}

// ---- What a key that shows something might do when pressed --------------------

type DoAction = Extract<Block, { block: "doAction" }>;

const SUGGESTIONS: readonly { reads: readonly string[]; label: string; plugin: string; action: string; args: Record<string, string> }[] = [
  { reads: ["system.cpu", "sensors.memory", "sensors.gpu"], label: "Open Task Manager", plugin: "launcher", action: "open_app", args: { app: "taskmgr" } },
  { reads: ["sensors.network", "sensors.disk"], label: "Open Resource Monitor", plugin: "launcher", action: "open_app", args: { app: "resmon" } },
  { reads: ["sensors.battery"], label: "Open battery settings", plugin: "launcher", action: "open_app", args: { app: "ms-settings:batterysaver" } },
  { reads: ["media.current"], label: "Play / pause", plugin: "media", action: "play_pause", args: {} },
  { reads: ["time."], label: "Open Clock", plugin: "launcher", action: "open_app", args: { app: "ms-clock:" } },
];

/**
 * A press worth suggesting for a key, from the values its face shows: a CPU
 * gauge opens Task Manager. Only what this desktop can do is offered.
 */
export function suggestedActions(widget: WireWidget, catalogue: readonly { plugin: string; action: string }[]): { label: string; block: DoAction }[] {
  const shows = JSON.stringify({ face: widget.face, props: widget.props, states: widget.states?.variable });
  return SUGGESTIONS.filter((s) => s.reads.some((prefix) => shows.includes(prefix)) && catalogue.some((entry) => entry.plugin === s.plugin && entry.action === s.action)).map((s) => ({
    label: s.label,
    block: { block: "doAction", plugin: s.plugin, action: s.action, args: Object.fromEntries(Object.entries(s.args).map(([k, v]) => [k, { text: v }])) },
  }));
}

/** A key with nothing on it yet: no press, no face, no states, no name of its own. */
export function isBlankKey(widget: WireWidget): boolean {
  const label = widget.props?.label ?? "";
  return !widget.flows?.length && !widget.face && !widget.states && !widget.sources?.length && (label === "" || label === "New button") && !widget.props?.icon;
}

/**
 * Make a key the preset, all of it — its look and what it does — where the
 * key stands, keeping a name someone gave it. It grows to the preset's size
 * when there is room.
 */
export function applyWholePreset(config: WireConfig, widgetId: string, preset: Preset): WireConfig {
  const target = findWidget(config, widgetId)?.widget;
  if (!target || target.type === "WIDGET_TYPE_SLIDER") return config;
  const named = target.props?.label && target.props.label !== "New button" ? target.props.label : undefined;
  let next = withWidget(config, widgetId, (widget) => {
    const { face, flows, states, sources, props } = sharedReads(preset.widget);
    const made: WireWidget = { id: widget.id, type: widget.type ?? "WIDGET_TYPE_BUTTON", col: widget.col, row: widget.row, w: widget.w, h: widget.h };
    if (widget.portrait) made.portrait = widget.portrait;
    made.props = { ...(props ?? {}), label: named ?? props?.label ?? preset.name };
    if (face) made.face = face;
    if (flows?.length) made.flows = flows;
    if (states) made.states = states;
    if (sources?.length) made.sources = sources;
    return made;
  });
  if (presetFits(next, widgetId, preset)) next = resizeWidget(next, widgetId, preset.w, preset.h);
  return withSchemaFor(next);
}
