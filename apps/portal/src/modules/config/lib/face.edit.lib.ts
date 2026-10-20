import { derive, faceFor, type Background, type Face, type FaceElement, type FaceLayout, type Zone } from "@flexdeck/face";

import { type ScreenCare, type CaseOp, type WireCase, type WireConfig, type WireGestureLook, type WireRule, type WireLook, type WireWidget } from "../../../services/api/api.ts";
import { withSchemaFor } from "./config.lib.ts";
import { findWidget, setWidgetColors, withPage, withWidget } from "./editing.lib.ts";
import { toBackground, type Fill } from "./fill.lib.ts";

/**
 * Editing faces, looks and page backgrounds.
 *
 * Every edit here may write something only schema 4 (faces), 5 (gesture
 * states, value rules) or 6 (conditions) reads, so each one raises the document to what it holds: an agent that reads faces is then the one that
 * gets them, and an older one refuses the deck by name rather than dropping
 * them unread.
 */

function current(config: WireConfig): WireConfig {
  return withSchemaFor(config);
}

function editWidget(config: WireConfig, widgetId: string, update: (widget: WireWidget) => WireWidget): WireConfig {
  const next = withWidget(config, widgetId, update);
  return next === config ? config : current(next);
}

/** Whether the widget draws a face of its own, rather than its props. */
export function hasFace(widget: WireWidget): boolean {
  return widget.face !== undefined;
}

/**
 * Give a widget a face of its own, starting from the one its props draw so
 * nothing on the key changes yet — or, with `null`, go back to its props.
 */
export function setCustomFace(config: WireConfig, widgetId: string, on: boolean): WireConfig {
  return editWidget(config, widgetId, (widget) => {
    if (on) {
      return widget.face ? widget : { ...widget, face: derive(widget.props) };
    }
    const next = { ...widget };
    delete next.face;
    return next;
  });
}

/**
 * Change the face shown in `lookId` (`""`: the widget's own). A look without
 * a face of its own is given a copy of what it shows now, then changed.
 */
export function editFace(config: WireConfig, widgetId: string, lookId: string, change: (face: Face) => Face): WireConfig {
  return editWidget(config, widgetId, (widget) => {
    if (!lookId) {
      return { ...widget, face: change(structuredClone(widget.face ?? derive(widget.props))) };
    }
    const looks = widget.states?.looks ?? [];
    if (!looks.some((look) => look.id === lookId)) {
      return widget;
    }
    const base = faceFor(widget, lookId);
    return {
      ...widget,
      states: {
        ...widget.states,
        looks: looks.map((look) => (look.id === lookId ? { ...look, face: change(structuredClone(base)) } : look)),
      },
    };
  });
}

/**
 * Fill a key's face. A simple key keeps a plain colour in its props; a fade
 * or a picture needs a face, so it is given one first — drawn from its props,
 * so nothing else on the key changes.
 */
export function setKeyFill(config: WireConfig, widgetId: string, fill: Fill): WireConfig {
  const widget = findWidget(config, widgetId)?.widget;
  if (!widget) return config;
  if (!widget.face) {
    if (fill.kind === "solid" || fill.kind === "default") return setWidgetColors(config, widgetId, { bgColor: fill.kind === "solid" ? fill.color : "" });
    config = setCustomFace(config, widgetId, true);
  }
  return editFace(config, widgetId, "", (face) => setBackground(face, toBackground(fill, face.background) ?? {}));
}

/** Stop a look having its own face: it shows the widget's again, in its colours. */
export function clearLookFace(config: WireConfig, widgetId: string, lookId: string): WireConfig {
  return editWidget(config, widgetId, (widget) => ({
    ...widget,
    states: {
      ...widget.states,
      looks: (widget.states?.looks ?? []).map((look) => {
        if (look.id !== lookId) return look;
        const next = { ...look };
        delete next.face;
        return next;
      }),
    },
  }));
}

export function setLayout(face: Face, layout: FaceLayout): Face {
  const next: Face = { ...face, layout };
  if (layout === "FACE_LAYOUT_ICON_LABEL") delete next.layout;
  return next;
}

export function setBackground(face: Face, background: Background): Face {
  const cleaned = Object.fromEntries(
    Object.entries(background).filter(([, value]) => value !== undefined && value !== "" && value !== false && value !== 0),
  ) as Background;
  const next = { ...face };
  if (Object.keys(cleaned).length > 0) next.background = cleaned;
  else delete next.background;
  return next;
}

export function addElement(face: Face, element: FaceElement): Face {
  return { ...face, elements: [...(face.elements ?? []), element] };
}

export function updateElement(face: Face, index: number, element: FaceElement): Face {
  return { ...face, elements: (face.elements ?? []).map((existing, at) => (at === index ? element : existing)) };
}

export function removeElement(face: Face, index: number): Face {
  return { ...face, elements: (face.elements ?? []).filter((_, at) => at !== index) };
}

/** Move an element one place earlier (-1) or later (+1): later draws on top. */
export function moveElement(face: Face, index: number, delta: -1 | 1): Face {
  const elements = [...(face.elements ?? [])];
  const target = index + delta;
  if (target < 0 || target >= elements.length) return face;
  [elements[index], elements[target]] = [elements[target]!, elements[index]!];
  return { ...face, elements };
}

/** The element a new one in `kind` starts as: something that already shows. */
export function starterElement(kind: NonNullable<FaceElement["kind"]>, layout: FaceLayout | undefined): FaceElement {
  switch (kind) {
    case "ELEMENT_KIND_ICON":
      return { slot: "SLOT_ICON", kind, template: "bolt" };
    case "ELEMENT_KIND_BAR":
      return { slot: "SLOT_VISUAL", kind, template: "{{system.cpu.usage}}" };
    case "ELEMENT_KIND_GAUGE":
      return { slot: "SLOT_VISUAL", kind, template: "{{system.cpu.usage}}" };
    case "ELEMENT_KIND_GRAPH":
      return { slot: "SLOT_VISUAL", kind, variable: "system.cpu.usage" };
    case "ELEMENT_KIND_TIMER":
      return { slot: layout === "FACE_LAYOUT_DIAL" ? "SLOT_VISUAL" : "SLOT_VALUE", kind, variable: "focus" };
    case "ELEMENT_KIND_CLOCK":
      return { slot: layout === "FACE_LAYOUT_DIAL" ? "SLOT_VISUAL" : "SLOT_VALUE", kind, clock: layout === "FACE_LAYOUT_DIAL" ? { analog: true } : {} };
    default:
      return { slot: layout === "FACE_LAYOUT_VALUE" ? "SLOT_VALUE" : "SLOT_TITLE", template: "Text" };
  }
}

// ---- Looks: several faces, and what picks one ------------------------------

export type LookSource =
  | { kind: "none" }
  | { kind: "cycle" }
  | { kind: "variable"; variable: string; cases: WireCase[]; otherwise: string }
  | { kind: "gesture"; picks: WireGestureLook[] };

export function lookSourceOf(widget: WireWidget): LookSource {
  const states = widget.states;
  if (states?.cycle) return { kind: "cycle" };
  if (states?.variable) {
    return {
      kind: "variable",
      variable: states.variable.variable ?? "",
      cases: states.variable.cases ?? [],
      otherwise: states.variable.otherwise ?? "",
    };
  }
  if (states?.gesture) return { kind: "gesture", picks: states.gesture.looks ?? [] };
  return { kind: "none" };
}

/** A new look's id: `look-2`, `look-3`, … — never one the widget has. */
export function nextLookId(widget: WireWidget): string {
  const taken = new Set((widget.states?.looks ?? []).map((look) => look.id));
  for (let n = taken.size + 1; ; n += 1) {
    const id = `look-${n}`;
    if (!taken.has(id)) return id;
  }
}

/** Add a look, shown when picked; it starts as the widget's own face. */
export function addLook(config: WireConfig, widgetId: string, look: WireLook): WireConfig {
  return editWidget(config, widgetId, (widget) => ({
    ...widget,
    states: { ...widget.states, looks: [...(widget.states?.looks ?? []), look] },
  }));
}

export function updateLook(config: WireConfig, widgetId: string, lookId: string, change: Partial<WireLook>): WireConfig {
  return editWidget(config, widgetId, (widget) => ({
    ...widget,
    states: {
      ...widget.states,
      looks: (widget.states?.looks ?? []).map((look) => {
        if (look.id !== lookId) return look;
        const next: WireLook = { ...look, ...change };
        for (const key of ["label", "bgColor", "fgColor", "icon"] as const) {
          if (!next[key]?.trim()) delete next[key];
        }
        return next;
      }),
    },
  }));
}

/**
 * Remove a look, and every case that showed it; a fallback that named it
 * falls back to the widget's own face. With no looks left, so does the rule.
 */
export function removeLook(config: WireConfig, widgetId: string, lookId: string): WireConfig {
  return editWidget(config, widgetId, (widget) => {
    const looks = (widget.states?.looks ?? []).filter((look) => look.id !== lookId);
    if (looks.length === 0) {
      const next = { ...widget };
      delete next.states;
      return next;
    }
    const states = { ...widget.states, looks };
    if (states.gesture) states.gesture = { looks: (states.gesture.looks ?? []).filter((pick) => pick.state !== lookId) };
    if (states.rules) {
      const rules = states.rules.filter((rule) => rule.state !== lookId);
      if (rules.length) states.rules = rules;
      else delete states.rules;
    }
    if (states.variable) {
      states.variable = {
        ...states.variable,
        cases: (states.variable.cases ?? []).filter((c) => c.state !== lookId),
        otherwise: states.variable.otherwise === lookId ? "" : states.variable.otherwise,
      };
    }
    return { ...widget, states };
  });
}

/**
 * Set the looks a value or a condition picks ahead of the source: "while
 * obs.recording is true, show Recording", "while obs.preview_scene !=
 * obs.scene, show Ready". Blank rows are kept out; none left removes them.
 */
export function setLookRules(config: WireConfig, widgetId: string, rules: readonly WireRule[]): WireConfig {
  const kept = rules
    .filter((rule) => (rule.when !== undefined ? rule.when.trim() : rule.variable?.trim()) && rule.state)
    .map((rule) => {
      if (rule.when !== undefined) return { when: rule.when.trim(), state: rule.state! };
      const out: WireRule = { variable: rule.variable!.trim(), value: rule.value ?? "", state: rule.state! };
      if (rule.op && rule.op !== "COMPARE_OP_EQ") out.op = rule.op;
      return out;
    });
  // A row still being filled in changes nothing, and must not spend a revision.
  const before = findWidget(config, widgetId)?.widget.states?.rules ?? [];
  if (JSON.stringify(before) === JSON.stringify(kept)) return config;
  return editWidget(config, widgetId, (widget) => {
    const states = { ...widget.states, looks: widget.states?.looks ?? [] };
    if (kept.length) states.rules = kept;
    else delete states.rules;
    return { ...widget, states };
  });
}

/** Choose what picks the look. The looks themselves, and the rules, are kept. */
export function setLookSource(config: WireConfig, widgetId: string, source: LookSource): WireConfig {
  const found = findWidget(config, widgetId)?.widget;
  if (found && !found.states) return config;
  return editWidget(config, widgetId, (widget) => {
    const looks = widget.states?.looks ?? [];
    // Nothing to pick between: no states at all, rather than an empty set the
    // agent reads as a choice with nothing to choose.
    if (looks.length === 0) {
      const next = { ...widget };
      delete next.states;
      return next;
    }
    const states: NonNullable<WireWidget["states"]> = { looks };
    if (widget.states?.rules?.length) states.rules = widget.states.rules;
    if (source.kind === "cycle") states.cycle = {};
    if (source.kind === "gesture") states.gesture = { looks: source.picks.filter((pick) => pick.gesture && pick.state) };
    if (source.kind === "variable") {
      states.variable = {
        variable: source.variable.trim(),
        cases: source.cases.map((c) => {
          if (c.when !== undefined) return { when: c.when.trim(), state: c.state ?? "" };
          const out: WireCase = { value: c.value ?? "", state: c.state ?? "" };
          if (c.op && c.op !== "COMPARE_OP_EQ") out.op = c.op;
          return out;
        }),
        otherwise: source.otherwise,
      };
    }
    return { ...widget, states };
  });
}

export const CASE_OPS: readonly { op: CaseOp; label: string }[] = [
  { op: "COMPARE_OP_EQ", label: "is" },
  { op: "COMPARE_OP_NE", label: "is not" },
  { op: "COMPARE_OP_GT", label: "is above" },
  { op: "COMPARE_OP_GE", label: "is at least" },
  { op: "COMPARE_OP_LT", label: "is below" },
  { op: "COMPARE_OP_LE", label: "is at most" },
];

// ---- Pages ------------------------------------------------------------------

export function setPageBackground(config: WireConfig, pageId: string, background: Background | null): WireConfig {
  const next = withPage(config, pageId, (page) => {
    const updated = { ...page };
    const cleaned = background ? setBackground({}, background).background : undefined;
    if (cleaned) updated.background = cleaned;
    else delete updated.background;
    return updated;
  });
  return next === config ? config : current(next);
}

// ---- A key's own run of a data source ---------------------------------------

// ---- A face's strip of zones ---------------------------------------------------

/** The buttons a music key's strip starts with. */
export const MUSIC_ZONES: readonly Zone[] = [
  { name: "prev", icon: "skip-back" },
  { name: "play", icon: "play", when: "media.current.playing", iconOn: "pause" },
  { name: "next", icon: "skip-forward" },
];

/** A name for a new zone, unique on the face. */
export function zoneName(zones: readonly Zone[]): string {
  let n = zones.length + 1;
  while (zones.some((z) => z.name === `button${n}`)) n += 1;
  return `button${n}`;
}

/** `face` with `zones`; none: no strip at all. */
export function setZones(face: Face, zones: Zone[]): Face {
  const next = { ...face };
  if (zones.length > 0) next.zones = zones;
  else delete next.zones;
  return next;
}

/**
 * Rename a zone on a widget, its flows following it, or (`to` null) remove
 * it and the flows that were its own.
 */
export function renameZone(config: WireConfig, widgetId: string, from: string, to: string | null): WireConfig {
  return editWidget(config, widgetId, (widget) => {
    const zones = (widget.face?.zones ?? []).flatMap((z) => (z.name !== from ? [z] : to === null ? [] : [{ ...z, name: to }]));
    const flows = (widget.flows ?? []).flatMap((flow) => (flow.zone !== from ? [flow] : to === null ? [] : [{ ...flow, zone: to }]));
    const next = { ...widget, face: setZones(widget.face ?? {}, zones) };
    if (flows.length > 0) next.flows = flows;
    else delete next.flows;
    return next;
  });
}

// ---- The tablet's screen ------------------------------------------------------

/** Set the screen care of the profile that holds `pageId`. Unset fields are dropped. */
export function setScreenCare(config: WireConfig, pageId: string, care: ScreenCare): WireConfig {
  let changed = false;
  const profiles = (config.profiles ?? []).map((profile) => {
    if (!(profile.pages ?? []).some((page) => page.id === pageId)) return profile;
    changed = true;
    const cleaned = Object.fromEntries(Object.entries(care).filter(([, v]) => v !== undefined && v !== false && v !== 0)) as ScreenCare;
    const next = { ...profile };
    if (Object.keys(cleaned).length > 0) next.screen = cleaned;
    else delete next.screen;
    return next;
  });
  return changed ? current({ ...config, profiles }) : config;
}
