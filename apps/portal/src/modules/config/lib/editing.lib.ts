import type { GridSpec, WidgetSpan } from "@flexdeck/grid";

import type {
  ActionDefinition,
  Block,
  Gesture,
  ValueRef,
  WireAction,
  WireConfig,
  WireFlow,
  WirePage,
  WireProfile,
  WireStates,
  WireWidget,
} from "../../../services/api/api.ts";
import { gridSpecOf, pageOf, profileOf } from "./config.lib.ts";

/**
 * Editing operations on a configuration.
 *
 * Everything here is a pure function from one configuration to another. That is
 * deliberate: the interesting failures in an editor are not visual, they are
 * logical — a widget pushed out of bounds, two widgets on the same cell, a
 * resize below the minimum — and those are only reliably catchable if the rules
 * live somewhere that can be tested without a browser.
 *
 * Nothing here mutates. A rejected edit returns the configuration it was given,
 * so a caller cannot half-apply one by accident.
 */

/** How large a widget of a given type is allowed to be. */
export interface SpanLimits {
  minW: number;
  minH: number;
  maxW: number;
  maxH: number;
}

/// Per-type limits, from the shared contract. A type this build does not know
/// gets the permissive default rather than being rejected — a newer portal
/// should still be able to move a widget it cannot render.
const SPAN_LIMITS: Record<string, SpanLimits> = {
  WIDGET_TYPE_BUTTON: { minW: 1, minH: 1, maxW: 4, maxH: 4 },
  // A fader can run the whole height or width of a deck.
  WIDGET_TYPE_SLIDER: { minW: 1, minH: 1, maxW: 8, maxH: 8 },
};

export const DEFAULT_LIMITS: SpanLimits = { minW: 1, minH: 1, maxW: 8, maxH: 8 };

export function limitsFor(type: string | undefined): SpanLimits {
  return (type && SPAN_LIMITS[type]) || DEFAULT_LIMITS;
}

/** Why a placement was refused. */
export type PlacementProblem = "out_of_bounds" | "overlap" | "too_small" | "too_large";

export function describeProblem(problem: PlacementProblem): string {
  switch (problem) {
    case "out_of_bounds":
      return "that position is outside the grid";
    case "overlap":
      return "another widget is already there";
    case "too_small":
      return "that is smaller than this widget allows";
    case "too_large":
      return "that is larger than this widget allows";
  }
}

/**
 * Whether `candidate` may sit where it is, given the other widgets on the page.
 *
 * The candidate is checked against `others` rather than against every widget,
 * so a caller moving an existing widget passes the rest of the page — a widget
 * overlapping its own previous position is not a conflict.
 */
export function placementProblem(
  spec: GridSpec,
  others: readonly WidgetSpan[],
  candidate: WidgetSpan,
  limits: SpanLimits = DEFAULT_LIMITS,
): PlacementProblem | null {
  if (candidate.w < limits.minW || candidate.h < limits.minH) {
    return "too_small";
  }
  if (candidate.w > limits.maxW || candidate.h > limits.maxH) {
    return "too_large";
  }
  if (candidate.col < 0 || candidate.row < 0) {
    return "out_of_bounds";
  }
  if (candidate.col + candidate.w > spec.cols || candidate.row + candidate.h > spec.rows) {
    return "out_of_bounds";
  }
  if (others.some((other) => other.id !== candidate.id && overlaps(other, candidate))) {
    return "overlap";
  }
  return null;
}

function overlaps(a: WidgetSpan, b: WidgetSpan): boolean {
  return a.col < b.col + b.w && b.col < a.col + a.w && a.row < b.row + b.h && b.row < a.row + a.h;
}

/** Where a widget sits, and the page it sits on. */
export interface Located {
  profile: WireProfile;
  page: WirePage;
  widget: WireWidget;
}

export function findWidget(config: WireConfig | null, widgetId: string): Located | null {
  for (const profile of config?.profiles ?? []) {
    for (const page of profile.pages ?? []) {
      const widget = (page.widgets ?? []).find((candidate) => candidate.id === widgetId);
      if (widget) {
        return { profile, page, widget };
      }
    }
  }
  return null;
}

export function spansOf(page: WirePage | null): WidgetSpan[] {
  return (page?.widgets ?? []).map((widget) => ({
    id: widget.id,
    col: widget.col ?? 0,
    row: widget.row ?? 0,
    w: Math.max(1, widget.w ?? 1),
    h: Math.max(1, widget.h ?? 1),
  }));
}

/**
 * Rewrite the widget with `widgetId`, wherever it is.
 *
 * Returns the configuration unchanged when the id is not found, so a stale click
 * — a widget deleted in another tab — is a no-op rather than a crash.
 */
export function withWidget(
  config: WireConfig,
  widgetId: string,
  update: (widget: WireWidget) => WireWidget,
): WireConfig {
  // Checked FIRST, because `mapWidgets` rebuilds every profile and page and so
  // returns a structurally-identical but NEW object for an unknown id. That would
  // defeat the refusal contract: identity is how `applyEdit` tells a refusal from
  // a success, so a no-op would look like a change and spend a revision.
  if (!findWidget(config, widgetId)) {
    return config;
  }

  return mapWidgets(config, (widget) => (widget.id === widgetId ? update(widget) : widget));
}

function mapWidgets(config: WireConfig, update: (widget: WireWidget) => WireWidget): WireConfig {
  return {
    ...config,
    profiles: (config.profiles ?? []).map((profile) => ({
      ...profile,
      pages: (profile.pages ?? []).map((page) => ({
        ...page,
        widgets: (page.widgets ?? []).map(update),
      })),
    })),
  };
}

/** Move a widget to a new cell, keeping its span. Returns it unchanged if the
 *  destination is refused. */
export function moveWidget(config: WireConfig, widgetId: string, col: number, row: number): WireConfig {
  const found = findWidget(config, widgetId);
  if (!found) {
    return config;
  }

  const spec = gridSpecOf(found.profile);
  const others = spansOf(found.page);
  const candidate: WidgetSpan = {
    id: widgetId,
    col,
    row,
    w: found.widget.w ?? 1,
    h: found.widget.h ?? 1,
  };

  if (placementProblem(spec, others, candidate, limitsFor(found.widget.type)) !== null) {
    return config;
  }

  return withWidget(config, widgetId, (widget) => ({ ...widget, col, row }));
}

export function resizeWidget(config: WireConfig, widgetId: string, w: number, h: number): WireConfig {
  const found = findWidget(config, widgetId);
  if (!found) {
    return config;
  }

  const candidate: WidgetSpan = {
    id: widgetId,
    col: found.widget.col ?? 0,
    row: found.widget.row ?? 0,
    w,
    h,
  };

  if (placementProblem(gridSpecOf(found.profile), spansOf(found.page), candidate, limitsFor(found.widget.type)) !== null) {
    return config;
  }

  return withWidget(config, widgetId, (widget) => ({ ...widget, w, h }));
}

/** Set a widget's label. An empty label is stored as absent, matching what the
 *  agent sends back — a present-but-empty string would be a difference the
 *  round trip silently erases. */
export function setWidgetLabel(config: WireConfig, widgetId: string, label: string): WireConfig {
  return withWidget(config, widgetId, (widget) => {
    const trimmed = label.trim();
    const props = { ...(widget.props ?? {}) };

    if (trimmed) {
      props.label = trimmed;
    } else {
      delete props.label;
    }

    // proto3 JSON omits empty messages, so an empty props object is not what the
    // agent would have sent; dropping the key keeps the document identical to its
    // wire form.
    return withProps(widget, props);
  });
}

/**
 * Set or clear a widget's icon.
 *
 * The same shape rule as the label: a blank icon is stored as ABSENT and an empty
 * `props` object is dropped, matching what the agent would send back — so a save
 * that changed nothing produces no diff.
 */
export function setWidgetIcon(config: WireConfig, widgetId: string, icon: string | null): WireConfig {
  return withWidget(config, widgetId, (widget) => {
    const props = { ...(widget.props ?? {}) };
    const trimmed = icon?.trim();

    if (trimmed) {
      props.icon = trimmed;
    } else {
      delete props.icon;
    }

    return withProps(widget, props);
  });
}

/** The looks a "light up while" widget has: off, then on. */
const LIT_STATES: WireStates["looks"] = [{ id: "off" }, { id: "on", bgColor: "#b91c1c", fgColor: "#ffffff" }];

/**
 * The bool variable a widget lights up with, when its states are the simple
 * "on while true" kind this editor writes. A widget with richer states says
 * nothing here, rather than being flattened into it.
 */
export function stateVariableOf(widget: WireWidget): string {
  const states = widget.states;
  const looks = states?.looks ?? [];
  const simple =
    looks.length === 2 &&
    looks[0]?.id === "off" &&
    looks[1]?.id === "on" &&
    (states?.variable?.cases?.length ?? 0) === 1 &&
    states?.variable?.cases?.[0]?.value === "true";
  return simple ? (states?.variable?.variable ?? "") : "";
}

/**
 * Bind a widget to live values, or unbind it: `stateVariable` lights it while a
 * bool is true — states "off" and "on", picked by that variable — and
 * `labelTemplate` makes its label follow values. Blank clears, and is stored
 * as absent, like every other prop.
 */
export function setWidgetBinding(
  config: WireConfig,
  widgetId: string,
  binding: { stateVariable: string; labelTemplate: string },
): WireConfig {
  return withWidget(config, widgetId, (widget) => {
    const props = { ...(widget.props ?? {}) };
    const state = binding.stateVariable.trim();
    const template = binding.labelTemplate.trim();
    if (template) props.labelTemplate = template;
    else delete props.labelTemplate;
    let next = withProps(widget, props);
    if (state) {
      next = {
        ...next,
        states: {
          looks: LIT_STATES,
          variable: { variable: state, cases: [{ value: "true", state: "on" }], otherwise: "off" },
        },
      };
    } else if (stateVariableOf(widget)) {
      // Only the simple binding this editor made is removed; richer states
      // are left for the editor that understands them.
      next = { ...next };
      delete next.states;
    }
    return next;
  });
}

/** Set or clear a widget's background and foreground colours. */
export function setWidgetColors(
  config: WireConfig,
  widgetId: string,
  colors: { bgColor?: string; fgColor?: string },
): WireConfig {
  return withWidget(config, widgetId, (widget) => {
    const props = { ...(widget.props ?? {}) };
    applyColor(props, "bgColor", colors.bgColor);
    applyColor(props, "fgColor", colors.fgColor);
    return withProps(widget, props);
  });
}

type WidgetProps = NonNullable<WireWidget["props"]>;

function applyColor(props: WidgetProps, key: "bgColor" | "fgColor", value: string | undefined): void {
  // An ABSENT colour means "leave this one alone"; only a present-but-blank value
  // clears it. Otherwise setting the background would wipe the foreground.
  if (value === undefined) {
    return;
  }

  const trimmed = value.trim();
  if (trimmed) {
    props[key] = trimmed;
  } else {
    delete props[key];
  }
}

/// Keep `props` only when it has something in it: proto3 omits empty messages.
///
/// The key is DELETED rather than set to `undefined`, so the document in memory
/// is identical to the document on the wire. `{ props: undefined }` serialises
/// the same, but it is not the same object — and `"props" in widget` would
/// disagree with what the agent sent.
function withProps(widget: WireWidget, props: WidgetProps): WireWidget {
  if (Object.keys(props).length > 0) {
    return { ...widget, props };
  }
  const next = { ...widget };
  delete next.props;
  return next;
}

const SHORT_PRESS: Gesture = "GESTURE_SHORT_PRESS";

/** The flow a gesture runs on this widget, if it has one. */
export function flowFor(widget: WireWidget, gesture: Gesture): WireFlow | null {
  return widget.flows?.find((flow) => flow.trigger?.gesture === gesture) ?? null;
}

/** A step's value as the single-action form shows it: text, or `{{name}}`. */
function valueText(value: ValueRef): string {
  if ("text" in value) return value.text;
  if ("number" in value) return String(value.number);
  if ("bool" in value) return String(value.bool);
  if ("input" in value) return "";
  return `{{${value.variable}}}`;
}

/** Text from the single-action form as a step value. */
function textValue(text: string): ValueRef {
  const variable = /^\{\{\s*([^{}\s]+)\s*\}\}$/.exec(text.trim());
  return variable ? { variable: variable[1]! } : { text };
}

/**
 * What a short press on this widget does, when that is one action — the
 * shape the single-action form edits. A flow with more steps reads as its
 * first action.
 */
export function pressActionOf(widget: WireWidget): WireAction | null {
  const first = flowFor(widget, SHORT_PRESS)?.blocks?.find(
    (block): block is Extract<Block, { block: "doAction" }> => block.block === "doAction",
  );
  if (!first) return null;
  return {
    plugin: first.plugin,
    action: first.action,
    args: Object.fromEntries(Object.entries(first.args ?? {}).map(([name, value]) => [name, valueText(value)])),
  };
}

/**
 * The `<plugin>:<action>` key the action picker uses, for a stored action.
 *
 * A widget stores the plugin and the action as two fields; the picker needs
 * them as one key. An action saved by an older build may have no plugin: it is
 * matched by name when exactly one plugin has an action of that name, and
 * otherwise left as it is, so the picker can say it does not recognise it.
 */
export function pressActionKey(action: WireAction | null, catalogue: readonly ActionDefinition[]): string {
  if (!action?.action) return "";
  if (action.plugin) return `${action.plugin}:${action.action}`;
  const matches = catalogue.filter((entry) => entry.action === action.action);
  const only = matches.length === 1 ? matches[0] : undefined;
  return only ? `${only.plugin}:${only.action}` : action.action;
}

/**
 * Set or clear what a short press does, as one action.
 *
 * The widget's other flows — long press, double tap, event triggers — are
 * kept. A widget left with no flows has none stored, matching what the agent
 * sends back: proto3 JSON omits an empty list, so keeping it would make every
 * save look like a change.
 */
export function setWidgetAction(
  config: WireConfig,
  widgetId: string,
  action: WireAction | null,
): WireConfig {
  return withWidget(config, widgetId, (widget) => {
    const others = (widget.flows ?? []).filter((flow) => flow.trigger?.gesture !== SHORT_PRESS);
    const flows: WireFlow[] = action
      ? [
          {
            trigger: { gesture: SHORT_PRESS },
            blocks: [
              {
                block: "doAction",
                plugin: action.plugin ?? "",
                action: action.action ?? "",
                args: Object.fromEntries(Object.entries(action.args ?? {}).map(([name, text]) => [name, textValue(text)])),
              },
            ],
          },
          ...others,
        ]
      : others;
    const next = { ...widget };
    if (flows.length > 0) next.flows = flows;
    else delete next.flows;
    return next;
  });
}

/** The looks a key lit while its action is active has: off, then the deck's own glow. */
const ACTIVE_STATES: WireStates["looks"] = [{ id: "off" }, { id: "on" }];

/** Whether these are the looks `setActiveLight` wrote: plain off/on, matched to one value. */
function isActiveStates(states: WireStates | undefined): boolean {
  const looks = states?.looks ?? [];
  return (
    looks.length === 2 &&
    looks[0]?.id === "off" &&
    looks[1]?.id === "on" &&
    !looks[1]?.bgColor &&
    (states?.variable?.cases?.length ?? 0) === 1 &&
    states?.variable?.cases?.[0]?.value !== "true"
  );
}

/**
 * Light a key while what its press picks is the one picked — the scene key
 * while that scene is live — when the action says how to tell (`active`).
 * States someone set up themselves are left alone; the ones this wrote follow
 * the action, and go when it no longer says.
 */
export function setActiveLight(
  config: WireConfig,
  widgetId: string,
  definition: ActionDefinition | null,
  args: Record<string, string>,
): WireConfig {
  return withWidget(config, widgetId, (widget) => {
    const ours = !widget.states || isActiveStates(widget.states);
    const value = definition?.active ? (args[definition.active.equals] ?? "").trim() : "";
    // A value still waiting on a placeholder never equals what the variable holds.
    if (definition?.active && value && !value.includes("{{")) {
      if (!ours) return widget;
      return {
        ...widget,
        states: {
          looks: ACTIVE_STATES,
          variable: { variable: definition.active.variable, cases: [{ value, state: "on" }], otherwise: "off" },
        },
      };
    }
    if (!widget.states || !isActiveStates(widget.states)) return widget;
    const next = { ...widget };
    delete next.states;
    return next;
  });
}

/**
 * Bring a key's light in line with what its press now does — after its tap
 * was changed by hand: a new scene lights for that scene, a press that can no
 * longer tell loses the light this wrote. Looks set up by hand stay.
 */
export function relightActive(config: WireConfig, widgetId: string, catalogue: readonly ActionDefinition[]): WireConfig {
  const widget = findWidget(config, widgetId)?.widget;
  if (!widget) return config;
  const action = pressActionOf(widget);
  const definition = action ? (catalogue.find((entry) => entry.plugin === action.plugin && entry.action === action.action) ?? null) : null;
  return setActiveLight(config, widgetId, definition, action?.args ?? {});
}

/**
 * Give every key whose press can be seen to be active — a scene key made
 * before actions said how to tell — the light `setActiveLight` gives a new
 * one. Keys with states of their own are left alone. The SAME config when
 * nothing needed it, so a caller can tell there is nothing to save.
 */
export function lightActiveKeys(config: WireConfig, catalogue: readonly ActionDefinition[]): WireConfig {
  let next = config;
  for (const profile of config.profiles ?? []) {
    for (const page of profile.pages ?? []) {
      for (const widget of page.widgets ?? []) {
        if (widget.states || !widget.id) continue;
        const action = pressActionOf(widget);
        const definition = catalogue.find((entry) => entry.plugin === action?.plugin && entry.action === action?.action);
        if (!action || !definition?.active) continue;
        const lit = setActiveLight(next, widget.id, definition, action.args ?? {});
        if (findWidget(lit, widget.id)?.widget.states) next = lit;
      }
    }
  }
  return next;
}

/**
 * Remove a widget.
 *
 * `PUT /api/config` replaces the whole document, so a widget is deleted by
 * omitting it — which is why this rebuilds the page's widget list rather than
 * marking anything.
 */
export function removeWidget(config: WireConfig, widgetId: string): WireConfig {
  return {
    ...config,
    profiles: (config.profiles ?? []).map((profile) => ({
      ...profile,
      pages: (profile.pages ?? []).map((page) => ({
        ...page,
        widgets: (page.widgets ?? []).filter((widget) => widget.id !== widgetId),
      })),
    })),
  };
}

/** The first cell that can hold a `w × h` widget, scanning left to right and
 *  then down. `null` when the page has no room. */
export function firstFreeCell(
  spec: GridSpec,
  spans: readonly WidgetSpan[],
  w = 1,
  h = 1,
): { col: number; row: number } | null {
  for (let row = 0; row + h <= spec.rows; row += 1) {
    for (let col = 0; col + w <= spec.cols; col += 1) {
      const candidate: WidgetSpan = { id: "", col, row, w, h };
      if (!spans.some((span) => overlaps(span, candidate))) {
        return { col, row };
      }
    }
  }
  return null;
}

/**
 * Add a widget to the first page of the first profile.
 *
 * Returns the configuration unchanged when there is no room or no page to add
 * it to — an editor that silently dropped the widget somewhere invalid would be
 * worse than one that visibly declined.
 */
export function addWidget(
  config: WireConfig,
  widget: Omit<WireWidget, "id">,
  pageId?: string,
): WireConfig {
  const profile = profileOf(config);
  const page = pageId ? findPage(config, pageId) : pageOf(profile);
  if (!profile || !page) {
    return config;
  }

  const spec = gridSpecOf(profile);
  const spans = spansOf(page);
  const limits = limitsFor(widget.type);
  const cell = firstFreeCell(spec, spans, limits.minW, limits.minH);
  if (!cell) {
    return config;
  }

  const placed: WireWidget = {
    ...widget,
    id: nextWidgetId(widgetIdsOf(config)),
    col: cell.col,
    row: cell.row,
    w: limits.minW,
    h: limits.minH,
  };

  return withPage(config, page.id, (target) => ({
    ...target,
    widgets: [...(target.widgets ?? []), placed],
  }));
}

/// A fresh id that does not collide with the ones already in use.
///
/// Sequential rather than random: an id a human can recognise makes the raw JSON
/// and the diffs readable, and uniqueness only has to hold within a
/// configuration.
/// Every widget id in the document, across all profiles and pages — the set a
/// new id must avoid, since the desktop refuses a configuration that repeats one.
export function widgetIdsOf(config: WireConfig): string[] {
  return (config.profiles ?? []).flatMap((profile) =>
    (profile.pages ?? []).flatMap((page) => (page.widgets ?? []).map((widget) => widget.id)),
  );
}

export function nextWidgetId(existing: readonly string[]): string {
  for (let n = 1; ; n += 1) {
    const candidate = `widget-${n}`;
    if (!existing.includes(candidate)) {
      return candidate;
    }
  }
}

/**
 * Add a widget to a NAMED cell rather than to the first free one.
 *
 * This is what the "+" on an empty cell means, so it must land exactly there.
 * Returns the configuration unchanged when that cell cannot hold the widget —
 * occupied, or off the grid — the same refusal contract as everything else here,
 * so the caller never has to guess whether it worked.
 */
export function addWidgetAt(
  config: WireConfig,
  pageId: string,
  col: number,
  row: number,
  widget: Omit<WireWidget, "id">,
): WireConfig {
  const profile = profileOf(config);
  const page = findPage(config, pageId);
  if (!profile || !page) {
    return config;
  }

  const limits = limitsFor(widget.type);
  const spans = spansOf(page);
  // The empty id cannot collide with a real one, so `placementProblem` checks
  // this candidate against every existing widget.
  const candidate: WidgetSpan = { id: "", col, row, w: limits.minW, h: limits.minH };
  if (placementProblem(gridSpecOf(profile), spans, candidate, limits) !== null) {
    return config;
  }

  const placed: WireWidget = {
    ...widget,
    id: nextWidgetId(widgetIdsOf(config)),
    col,
    row,
    w: limits.minW,
    h: limits.minH,
  };

  return withPage(config, page.id, (target) => ({
    ...target,
    widgets: [...(target.widgets ?? []), placed],
  }));
}

/**
 * Rewrite the page with `pageId`, wherever it is.
 *
 * Returns the configuration unchanged when the id is not found, so a stale click
 * — a page deleted in another tab — is a no-op rather than a crash.
 */
export function withPage(
  config: WireConfig,
  pageId: string,
  update: (page: WirePage) => WirePage,
): WireConfig {
  let found = false;

  const profiles = (config.profiles ?? []).map((profile) => {
    const pages = profile.pages ?? [];
    if (!pages.some((page) => page.id === pageId)) {
      return profile;
    }
    found = true;
    return {
      ...profile,
      pages: pages.map((page) => (page.id === pageId ? update(page) : page)),
    };
  });

  return found ? { ...config, profiles } : config;
}

/// The page with this id anywhere in the document, or `null`. Unlike
/// `pageById` this does not need a profile, because a page id alone is enough.
export function findPage(config: WireConfig | null, pageId: string): WirePage | null {
  for (const profile of config?.profiles ?? []) {
    const page = (profile.pages ?? []).find((candidate) => candidate.id === pageId);
    if (page) {
      return page;
    }
  }
  return null;
}

/// A fresh page id that does not collide with the ones already in use.
export function nextPageId(existing: readonly string[]): string {
  for (let n = 1; ; n += 1) {
    const candidate = `page-${n}`;
    if (!existing.includes(candidate)) {
      return candidate;
    }
  }
}

/**
 * Append a page.
 *
 * The id is the CALLER's to choose (via `nextPageId`) so a caller that wants to
 * select the new page can — without this function having to report an id back
 * through a return type that is otherwise always a configuration.
 */
export function addPage(config: WireConfig, pageId: string, name?: string): WireConfig {
  const profile = profileOf(config);
  if (!profile) {
    return config;
  }

  const pages = profile.pages ?? [];
  const page: WirePage = { id: pageId };
  if (name) {
    page.name = name;
  }

  return {
    ...config,
    profiles: (config.profiles ?? []).map((candidate) =>
      candidate.id === profile.id ? { ...candidate, pages: [...pages, page] } : candidate,
    ),
  };
}

/**
 * Remove a page.
 *
 * Refuses to remove the LAST page: a profile with no pages has nowhere to put a
 * widget, and the editor would have nothing to show. That is a refusal, not an
 * error — the caller gets its own configuration back and can say why.
 */
export function removePage(config: WireConfig, pageId: string): WireConfig {
  const profile = profileOf(config);
  if (!profile) {
    return config;
  }

  const pages = profile.pages ?? [];
  if (pages.length <= 1) {
    return config;
  }

  const remaining = pages.filter((page) => page.id !== pageId);
  if (remaining.length === pages.length) {
    return config;
  }

  return {
    ...config,
    profiles: (config.profiles ?? []).map((candidate) =>
      candidate.id === profile.id ? { ...candidate, pages: remaining } : candidate,
    ),
  };
}

/**
 * Rename a page.
 *
 * A blank name is stored as ABSENT, matching what the agent would send back
 * (proto3 omits defaults), so an emptied name does not look like a change on the
 * next save. Returns the configuration unchanged when the name already matches,
 * because committing an untouched field must not spend a revision.
 */
export function renamePage(config: WireConfig, pageId: string, name: string): WireConfig {
  const trimmed = name.trim();
  const page = findPage(config, pageId);
  if (!page || (page.name ?? "") === trimmed) {
    return config;
  }

  return withPage(config, pageId, (target) => {
    const next = { ...target };
    if (trimmed) {
      next.name = trimmed;
    } else {
      delete next.name;
    }
    return next;
  });
}

/**
 * Move a page one slot earlier or later.
 *
 * Refuses at the ends rather than wrapping, so "move up" on the first page does
 * nothing visible instead of silently jumping it to the bottom.
 */
export function movePage(config: WireConfig, pageId: string, delta: number): WireConfig {
  const profile = profileOf(config);
  if (!profile) {
    return config;
  }

  const pages = profile.pages ?? [];
  const from = pages.findIndex((page) => page.id === pageId);
  const to = from + delta;
  if (from < 0 || to < 0 || to >= pages.length) {
    return config;
  }

  const reordered = [...pages];
  const [moved] = reordered.splice(from, 1);
  reordered.splice(to, 0, moved);

  return {
    ...config,
    profiles: (config.profiles ?? []).map((candidate) =>
      candidate.id === profile.id ? { ...candidate, pages: reordered } : candidate,
    ),
  };
}

/**
 * Duplicate a page, directly after the original.
 *
 * Widget ids are remapped: ids are unique across the whole document, and a copy
 * that shared them would make `findWidget` ambiguous — pressing a button would
 * act on whichever the search found first.
 */
export function duplicatePage(config: WireConfig, pageId: string, newPageId: string): WireConfig {
  const profile = profileOf(config);
  const page = findPage(config, pageId);
  if (!profile || !page) {
    return config;
  }

  const pages = profile.pages ?? [];
  const taken = widgetIdsOf(config);

  const widgets = (page.widgets ?? []).map((widget) => {
    const id = nextWidgetId(taken);
    taken.push(id);
    return { ...widget, id };
  });

  const copy: WirePage = { id: newPageId };
  if (page.name) {
    copy.name = `${page.name} copy`;
  }
  if (widgets.length > 0) {
    copy.widgets = widgets;
  }

  const index = pages.findIndex((candidate) => candidate.id === pageId);
  const next = [...pages.slice(0, index + 1), copy, ...pages.slice(index + 1)];

  return {
    ...config,
    profiles: (config.profiles ?? []).map((candidate) =>
      candidate.id === profile.id ? { ...candidate, pages: next } : candidate,
    ),
  };
}

/**
 * Duplicate a widget into its page's first free cell.
 *
 * Returns the configuration unchanged when the page has no room: a duplicate that
 * silently landed on top of something would be worse than one that visibly
 * declined. The copy gets a fresh id, because ids are unique across the document
 * and a shared one would make `findWidget` ambiguous.
 */
export function duplicateWidget(config: WireConfig, widgetId: string): WireConfig {
  const found = findWidget(config, widgetId);
  if (!found) {
    return config;
  }

  const spec = gridSpecOf(found.profile);
  const spans = spansOf(found.page);
  const w = Math.max(1, found.widget.w ?? 1);
  const h = Math.max(1, found.widget.h ?? 1);
  const cell = firstFreeCell(spec, spans, w, h);
  if (!cell) {
    return config;
  }

  const copy: WireWidget = {
    ...found.widget,
    id: nextWidgetId(widgetIdsOf(config)),
    col: cell.col,
    row: cell.row,
    w,
    h,
  };

  return withPage(config, found.page.id, (page) => ({
    ...page,
    widgets: [...(page.widgets ?? []), copy],
  }));
}
