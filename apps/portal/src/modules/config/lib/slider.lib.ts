/**
 * Sliders and holds, as edits to a configuration.
 *
 * A slider widget runs one flow whenever it settles on a value, and the value
 * reaches the flow's step as the run's input `value`. The editor never shows
 * that plumbing: it offers the actions a slider can drive (the ones whose
 * catalogue entry carries `slider` metadata), and choosing one writes the flow,
 * the range, and the variable the thumb follows — the plugin's `feedback`
 * template with the other arguments filled in, so a slider on Spotify's volume
 * follows `system.audio.app.spotify.exe.volume`.
 *
 * A hold is a button with two flows, one for the finger going down and one for
 * it lifting — push-to-talk. The agent lifts it for a tablet that vanishes.
 */

import type {
  ActionDefinition,
  ArgumentChoices,
  Block,
  SliderOrientation,
  ValueRef,
  WireConfig,
  WireFlow,
  WireSlider,
  WireStates,
  WireWidget,
} from "../../../services/api/api.ts";
import { findWidget, resizeWidget, withWidget } from "./editing.lib.ts";

export const SLIDER_TYPE = "WIDGET_TYPE_SLIDER";
export const BUTTON_TYPE = "WIDGET_TYPE_BUTTON";

const HOLD_START = "GESTURE_HOLD_START";
const HOLD_END = "GESTURE_HOLD_END";
const TAP = "GESTURE_SHORT_PRESS";

/** The looks a slider with a mute gets: live, and muted. */
const LIVE = "live";
const MUTED = "muted";

/** One action and its arguments, as text — what a step picker edits. */
export interface Step {
  plugin: string;
  action: string;
  args: Record<string, string>;
}

/** Where a slider's thumb sits and the number beside its name, as the tablet draws them. */
export interface SliderReading {
  value: number;
  /** 0 to 1 along the track. */
  fraction: number;
  text: string;
}

/**
 * Read a slider the way the tablet does: the agent's live value, clamped into
 * range and onto a step; the bottom of the range while the value is unknown.
 */
export function sliderReading(slider: WireSlider | undefined, live: number | undefined): SliderReading {
  const min = slider?.min ?? 0;
  const max = slider?.max ?? 100;
  const step = slider?.step ?? 0;
  let value = live === undefined || Number.isNaN(live) ? min : Math.min(max, Math.max(min, live));
  if (step > 0) value = Math.min(max, min + Math.round((value - min) / step) * step);
  const fraction = max > min ? (value - min) / (max - min) : 0;
  const text = Number.isInteger(value) ? String(value) : value.toFixed(1);
  return { value, fraction, text };
}

/** Whether this widget is a slider. */
export function isSlider(widget: WireWidget): boolean {
  return widget.type === SLIDER_TYPE;
}

/** The actions a slider can drive, in catalogue order. */
export function sliderTargets(catalogue: readonly ActionDefinition[]): ActionDefinition[] {
  return catalogue.filter((action) => action.slider !== undefined);
}

/** Fill `{{name}}` placeholders from `args`; one with no value stays as it is. */
export function fillTemplate(template: string, args: Record<string, string>): string {
  return template.replace(/\{\{\s*([^{}\s]+)\s*\}\}/g, (whole, name: string) => {
    const value = args[name]?.trim();
    return value ? value : whole;
  });
}

/** Text from a form as a step value: `{{name}}` reads a variable. */
function textValue(text: string): ValueRef {
  const variable = /^\{\{\s*([^{}\s]+)\s*\}\}$/.exec(text.trim());
  return variable ? { variable: variable[1]! } : { text };
}

/** A step value as a form shows it. */
function valueText(value: ValueRef): string {
  if ("text" in value) return value.text;
  if ("number" in value) return String(value.number);
  if ("bool" in value) return String(value.bool);
  if ("variable" in value) return `{{${value.variable}}}`;
  return "";
}

function doAction(step: Step, overrides: Record<string, ValueRef> = {}): Block {
  return {
    block: "doAction",
    plugin: step.plugin,
    action: step.action,
    args: {
      ...Object.fromEntries(
        Object.entries(step.args)
          .filter(([, text]) => text.trim() !== "")
          .map(([name, text]) => [name, textValue(text)]),
      ),
      ...overrides,
    },
  };
}

function firstAction(flow: WireFlow | undefined): Step | null {
  const block = flow?.blocks?.find(
    (candidate): candidate is Extract<Block, { block: "doAction" }> => candidate.block === "doAction",
  );
  if (!block) return null;
  return {
    plugin: block.plugin,
    action: block.action,
    args: Object.fromEntries(
      Object.entries(block.args ?? {})
        // The slider's own argument reads its input; the form does not show it.
        .filter(([, value]) => !("input" in value))
        .map(([name, value]) => [name, valueText(value)]),
    ),
  };
}

/** What a slider drives, read back for the form. */
export function sliderTargetOf(widget: WireWidget): Step | null {
  return firstAction(widget.flows?.find((flow) => flow.trigger?.value !== undefined));
}

/**
 * Make a slider drive `step` — an action with slider metadata in `catalogue` —
 * or drive nothing (`null`). The range and the followed variable come from the
 * action; a range the person already narrowed is kept.
 */
export function setSliderTarget(
  config: WireConfig,
  widgetId: string,
  step: Step | null,
  catalogue: readonly ActionDefinition[],
): WireConfig {
  const definition = step
    ? catalogue.find((entry) => entry.plugin === step.plugin && entry.action === step.action)
    : undefined;
  const target = definition?.slider;
  if (step && !target) return config;

  return withWidget(config, widgetId, (widget) => {
    // A slider's tap is its mute, so it goes with the channel it mutes.
    const others = (widget.flows ?? []).filter(
      (flow) => flow.trigger?.value === undefined && flow.trigger?.gesture !== TAP,
    );
    const next: WireWidget = { ...widget };
    if (isMuteStates(next.states)) delete next.states;
    if (step && target) {
      const args = { ...step.args };
      delete args[target.argument];
      const mute = muteOf(definition!, args, catalogue);
      next.flows = [
        { trigger: { value: {} }, blocks: [doAction({ ...step, args }, { [target.argument]: { input: "value" } })] },
        ...(mute ? [{ trigger: { gesture: TAP }, blocks: [doAction(mute.step)] } as WireFlow] : []),
        ...others,
      ];
      if (mute?.variable && !next.states) {
        next.states = {
          looks: [{ id: LIVE }, { id: MUTED, icon: mutedIcon(next.props?.icon) }],
          variable: { variable: mute.variable, cases: [{ value: "true", state: MUTED }], otherwise: LIVE },
        };
      }
      const variable = target.feedback ? fillTemplate(target.feedback, args) : "";
      const slider: WireSlider = {
        ...widget.slider,
        min: target.min,
        max: target.max,
        step: target.step,
      };
      // A template that still has holes is not a variable that exists.
      if (variable && !variable.includes("{{")) slider.variable = variable;
      else delete slider.variable;
      next.slider = slider;
    } else {
      if (others.length > 0) next.flows = others;
      else delete next.flows;
      if (next.slider) {
        const slider = { ...next.slider };
        delete slider.variable;
        next.slider = slider;
      }
    }
    return next;
  });
}

/** Whether these are the live/muted looks a slider's channel wrote. */
function isMuteStates(states: WireStates | undefined): boolean {
  const ids = states?.looks?.map((look) => look.id) ?? [];
  return ids.length === 2 && ids[0] === LIVE && ids[1] === MUTED;
}

/** The icon a muted slider shows, by what it shows when live. */
function mutedIcon(icon: string | undefined): string {
  return icon === "mic" ? "mic-off" : "mute";
}

/**
 * How the channel `definition` drives, with `args`, is muted: the plugin's
 * toggle with the arguments it shares, and the variable saying so. `null`
 * when the plugin offers no mute, or names an action it does not have.
 */
function muteOf(
  definition: ActionDefinition,
  args: Record<string, string>,
  catalogue: readonly ActionDefinition[],
): { step: Step; variable: string | null } | null {
  const mute = definition.slider?.mute;
  if (!mute) return null;
  const toggle = catalogue.find((entry) => entry.plugin === definition.plugin && entry.action === mute.action);
  if (!toggle) return null;
  const shared = Object.fromEntries(
    toggle.arguments
      .filter((argument) => args[argument.name] !== undefined)
      .map((argument) => [argument.name, args[argument.name]!]),
  );
  const variable = mute.feedback ? fillTemplate(mute.feedback, args) : "";
  return {
    step: { plugin: definition.plugin, action: toggle.action, args: shared },
    variable: variable && !variable.includes("{{") ? variable : null,
  };
}

/**
 * One thing a slider can control, ready to pick: Master, the headphones,
 * OBS Studio, the Yeti. A slider action with nothing else to choose is one
 * channel; one with an argument the plugin can list is one channel per entry.
 */
export interface Channel {
  /** Stable across reloads: plugin, action, and the chosen value. */
  key: string;
  /** What the picker groups it under. */
  group: string;
  /** What the picker shows, and the name a fresh slider takes. */
  label: string;
  /** A bundled icon for a fresh slider. */
  icon: string;
  step: Step;
}

const keyOfStep = (step: Step) =>
  `${step.plugin}:${step.action}:${JSON.stringify(
    Object.entries(step.args)
      .filter(([, value]) => value.trim() !== "")
      .sort(([a], [b]) => a.localeCompare(b)),
  )}`;

/** The icon that suits a channel: a microphone for anything heard, else a speaker. */
function channelIcon(action: ActionDefinition, argument?: string): string {
  const words = `${action.action} ${action.description} ${argument ?? ""}`.toLowerCase();
  return /\b(input|mic|microphone)/.test(words) ? "mic" : "volume";
}

/** The names a widget has before anyone names it. */
const UNNAMED = ["", "Slider", "New button"];

/** A device's name without the "(default)" its choice label adds. */
const plainLabel = (label: string) => label.replace(/ \(default\)$/, "");

/**
 * Every channel the slider targets offer, given what each action's plugin
 * listed for its arguments (`choices`, by `plugin:action`). An action that
 * needs an argument nobody listed offers no channel; it stays reachable by
 * hand.
 */
export function sliderChannels(
  targets: readonly ActionDefinition[],
  choices: Readonly<Record<string, ArgumentChoices | undefined>>,
): Channel[] {
  const channels: Channel[] = [];
  for (const action of targets) {
    const plugin = action.pluginName ?? action.plugin;
    const needed = action.arguments.filter(
      (argument) => argument.name !== action.slider?.argument && !argument.optional,
    );
    if (needed.length === 0) {
      const step = { plugin: action.plugin, action: action.action, args: {} };
      channels.push({
        key: keyOfStep(step),
        group: plugin,
        label: action.description || action.action,
        icon: channelIcon(action),
        step,
      });
      continue;
    }
    if (needed.length !== 1) continue;
    const argument = needed[0]!;
    const offered = choices[`${action.plugin}:${action.action}`]?.arguments[argument.name] ?? [];
    for (const choice of offered) {
      const step = { plugin: action.plugin, action: action.action, args: { [argument.name]: choice.value } };
      channels.push({
        key: keyOfStep(step),
        group: `${plugin} — ${action.description || action.action}`,
        label: choice.label || choice.value,
        icon: channelIcon(action, argument.description),
        step,
      });
    }
  }
  return channels;
}

/** The key of the channel a slider drives, comparable with `Channel.key`. */
export function channelKeyOf(widget: WireWidget): string | null {
  const step = sliderTargetOf(widget);
  return step ? keyOfStep(step) : null;
}

/**
 * Make a slider control `channel`: its value, its mute, and — while it still
 * has the name and icon of whatever it controlled before (or none) — the
 * channel's name and icon, so a fresh slider reads "OBS Studio" at once.
 */
export function setSliderChannel(
  config: WireConfig,
  widgetId: string,
  channel: Channel,
  catalogue: readonly ActionDefinition[],
  previous: Channel | null,
): WireConfig {
  const before = findWidget(config, widgetId)?.widget;
  const label = before?.props?.label?.trim() ?? "";
  const named = !UNNAMED.includes(label) && (!previous || label !== plainLabel(previous.label));
  const props = { ...before?.props };
  if (!named) props.label = plainLabel(channel.label);
  if (!props.icon || props.icon === previous?.icon) props.icon = channel.icon;
  const renamed = withWidget(config, widgetId, (widget) => ({ ...widget, props }));
  return setSliderTarget(renamed, widgetId, channel.step, catalogue);
}

/** Change a slider's direction, or the variable its thumb follows. */
export function setSliderLayout(
  config: WireConfig,
  widgetId: string,
  patch: { orientation?: SliderOrientation; variable?: string },
): WireConfig {
  return withWidget(config, widgetId, (widget) => {
    const slider: WireSlider = { ...widget.slider };
    if (patch.orientation !== undefined) {
      if (patch.orientation === "SLIDER_ORIENTATION_AUTO") delete slider.orientation;
      else slider.orientation = patch.orientation;
    }
    if (patch.variable !== undefined) {
      if (patch.variable.trim()) slider.variable = patch.variable.trim();
      else delete slider.variable;
    }
    return { ...widget, slider };
  });
}

/**
 * Turn a widget into a slider or back into a button. Its name, place and
 * looks stay; what it *does* is reset, because a press flow means nothing to a
 * slider and a value flow nothing to a button.
 */
export function setWidgetKind(config: WireConfig, widgetId: string, kind: "button" | "slider"): WireConfig {
  const switched = withWidget(config, widgetId, (widget) => {
    const wanted = kind === "slider" ? SLIDER_TYPE : BUTTON_TYPE;
    if ((widget.type ?? BUTTON_TYPE) === wanted) return widget;
    const next: WireWidget = { ...widget, type: wanted };
    delete next.flows;
    delete next.states;
    if (kind === "slider") next.slider = { min: 0, max: 100, step: 1 };
    else delete next.slider;
    return next;
  });
  if (kind !== "slider" || switched === config) return switched;

  // A slider wants room to move: two cells along one side. Taller first, then
  // wider — but only into free cells, never over a neighbour. With no room on
  // either side it stays one cell, which a slider still works in.
  const widget = findWidget(switched, widgetId)!.widget;
  const w = widget.w ?? 1;
  const h = widget.h ?? 1;
  if (w >= 2 || h >= 2) return switched;
  const taller = resizeWidget(switched, widgetId, w, 2);
  return taller !== switched ? taller : resizeWidget(switched, widgetId, 2, h);
}

/** Let a widget's icon follow a value — `{{media.tidal.art}}` — or stop. */
export function setIconTemplate(config: WireConfig, widgetId: string, template: string): WireConfig {
  const trimmed = template.trim();
  // Unchanged returns the same document, which is how the editor tells it spent nothing.
  if ((findWidget(config, widgetId)?.widget.props?.iconTemplate ?? "") === trimmed) return config;
  return withWidget(config, widgetId, (widget) => {
    const props = { ...widget.props };
    if (trimmed) props.iconTemplate = trimmed;
    else delete props.iconTemplate;
    return { ...widget, props };
  });
}

/** What a hold button does when pressed and when let go. */
export function holdStepsOf(widget: WireWidget): { start: Step | null; end: Step | null } {
  const flow = (gesture: string) => widget.flows?.find((candidate) => candidate.trigger?.gesture === gesture);
  return { start: firstAction(flow(HOLD_START)), end: firstAction(flow(HOLD_END)) };
}

/**
 * Set what a button does while held. Either step may be `null`. A button with
 * hold steps is pressed on the tablet as a hold, so its tap action is dropped
 * rather than left to fire as well.
 */
export function setHoldSteps(
  config: WireConfig,
  widgetId: string,
  start: Step | null,
  end: Step | null,
): WireConfig {
  return withWidget(config, widgetId, (widget) => {
    const keep = (widget.flows ?? []).filter((flow) => {
      const gesture = flow.trigger?.gesture;
      if (gesture === HOLD_START || gesture === HOLD_END) return false;
      // A tap is the hold's own first instant; both would fire.
      if ((start || end) && gesture === "GESTURE_SHORT_PRESS") return false;
      return true;
    });
    const flows: WireFlow[] = [
      ...(start ? [{ trigger: { gesture: HOLD_START }, blocks: [doAction(start)] } as WireFlow] : []),
      ...(end ? [{ trigger: { gesture: HOLD_END }, blocks: [doAction(end)] } as WireFlow] : []),
      ...keep,
    ];
    const next = { ...widget };
    if (flows.length > 0) next.flows = flows;
    else delete next.flows;
    return next;
  });
}
