import assert from "node:assert/strict";
import { test } from "node:test";

import type { ActionDefinition, WireConfig, WireWidget } from "../src/services/api/api.ts";
import { findWidget } from "../src/modules/config/lib/editing.lib.ts";
import {
  channelKeyOf,
  fillTemplate,
  holdStepsOf,
  setHoldSteps,
  setIconTemplate,
  setSliderChannel,
  setSliderLayout,
  setSliderTarget,
  setWidgetKind,
  sliderChannels,
  sliderTargetOf,
  sliderTargets,
} from "../src/modules/config/lib/slider.lib.ts";

const appVolume: ActionDefinition = {
  plugin: "system",
  action: "set_app_volume",
  description: "Set one program's volume",
  arguments: [
    { name: "app", description: "", example: "spotify.exe", kind: "text" },
    { name: "percent", description: "", example: "50", kind: "number" },
  ],
  slider: { argument: "percent", min: 0, max: 100, step: 1, feedback: "system.audio.app.{{app}}.volume" },
};

const userVolume: ActionDefinition = {
  plugin: "discord",
  action: "set_user_volume",
  description: "",
  arguments: [
    { name: "user", description: "", example: "", kind: "text" },
    { name: "volume", description: "", example: "100", kind: "number" },
  ],
  slider: { argument: "volume", min: 0, max: 200, step: 1, feedback: "discord.user.{{user}}.volume" },
};

const hotkey: ActionDefinition = {
  plugin: "core",
  action: "hotkey",
  description: "",
  arguments: [{ name: "keys", description: "", example: "ctrl+m", kind: "text" }],
};

const catalogue = [hotkey, appVolume, userVolume];

function configWith(widget: WireWidget): WireConfig {
  return {
    schema: 3,
    profiles: [{ id: "p", grid: { cols: 4, rows: 3, usableW: 400, usableH: 300 }, pages: [{ id: "home", widgets: [widget] }] }],
  };
}

const widgetIn = (config: WireConfig, id = "w") => findWidget(config, id)!.widget;

test("only actions with slider metadata are offered to a slider", () => {
  assert.deepEqual(sliderTargets(catalogue).map((action) => action.action), ["set_app_volume", "set_user_volume"]);
});

test("a template is filled from arguments, and a missing one stays a hole", () => {
  assert.equal(fillTemplate("system.audio.app.{{app}}.volume", { app: "spotify.exe" }), "system.audio.app.spotify.exe.volume");
  assert.equal(fillTemplate("discord.user.{{ user }}.volume", {}), "discord.user.{{ user }}.volume");
});

test("choosing a target writes the value flow, the range, and the followed variable", () => {
  const start = setWidgetKind(configWith({ id: "w", type: "WIDGET_TYPE_BUTTON", col: 0, row: 0 }), "w", "slider");
  const next = setSliderTarget(start, "w", { plugin: "system", action: "set_app_volume", args: { app: "spotify.exe", percent: "7" } }, catalogue);
  const widget = widgetIn(next);
  assert.equal(widget.type, "WIDGET_TYPE_SLIDER");
  assert.deepEqual(widget.flows, [
    {
      trigger: { value: {} },
      blocks: [
        {
          block: "doAction",
          plugin: "system",
          action: "set_app_volume",
          args: { app: { text: "spotify.exe" }, percent: { input: "value" } },
        },
      ],
    },
  ]);
  assert.equal(widget.slider?.variable, "system.audio.app.spotify.exe.volume");
  assert.deepEqual([widget.slider?.min, widget.slider?.max], [0, 100]);
  assert.deepEqual(sliderTargetOf(widget), { plugin: "system", action: "set_app_volume", args: { app: "spotify.exe" } });
});

test("a target whose variable cannot be filled yet follows nothing rather than a broken name", () => {
  const start = setWidgetKind(configWith({ id: "w", col: 0, row: 0 }), "w", "slider");
  const next = setSliderTarget(start, "w", { plugin: "discord", action: "set_user_volume", args: {} }, catalogue);
  assert.equal(widgetIn(next).slider?.variable, undefined);
  assert.equal(widgetIn(next).slider?.max, 200, "Discord's user volume goes to 200%");
});

test("an action that cannot drive a slider is refused, and clearing keeps the range", () => {
  const start = setWidgetKind(configWith({ id: "w", col: 0, row: 0 }), "w", "slider");
  assert.equal(setSliderTarget(start, "w", { plugin: "core", action: "hotkey", args: { keys: "a" } }, catalogue), start);
  const set = setSliderTarget(start, "w", { plugin: "system", action: "set_app_volume", args: { app: "x.exe" } }, catalogue);
  const cleared = widgetIn(setSliderTarget(set, "w", null, catalogue));
  assert.equal(cleared.flows, undefined);
  assert.equal(cleared.slider?.variable, undefined);
  assert.equal(cleared.slider?.max, 100);
});

test("a slider's direction and followed value can be set by hand", () => {
  const start = setWidgetKind(configWith({ id: "w", col: 0, row: 0 }), "w", "slider");
  const vertical = setSliderLayout(start, "w", { orientation: "SLIDER_ORIENTATION_VERTICAL", variable: " obs.input.Mic.volume " });
  assert.equal(widgetIn(vertical).slider?.orientation, "SLIDER_ORIENTATION_VERTICAL");
  assert.equal(widgetIn(vertical).slider?.variable, "obs.input.Mic.volume");
  const auto = setSliderLayout(vertical, "w", { orientation: "SLIDER_ORIENTATION_AUTO", variable: "" });
  assert.equal(widgetIn(auto).slider?.orientation, undefined, "auto is the default, so it is not stored");
  assert.equal(widgetIn(auto).slider?.variable, undefined);
});

test("turning a widget into a slider resets what it does and gives it room", () => {
  const button: WireWidget = {
    id: "w",
    type: "WIDGET_TYPE_BUTTON",
    col: 0,
    row: 0,
    props: { label: "Volume" },
    flows: [{ trigger: { gesture: "GESTURE_SHORT_PRESS" }, blocks: [] }],
  };
  const slider = widgetIn(setWidgetKind(configWith(button), "w", "slider"));
  assert.equal(slider.flows, undefined);
  assert.equal(slider.props?.label, "Volume", "its name stays");
  assert.equal(slider.h, 2);
  const back = widgetIn(setWidgetKind(setWidgetKind(configWith(button), "w", "slider"), "w", "button"));
  assert.equal(back.slider, undefined);
});

test("a slider only grows into free cells, never over a neighbour", () => {
  const page = (...widgets: WireWidget[]): WireConfig => ({
    schema: 3,
    profiles: [{ id: "p", grid: { cols: 4, rows: 3, usableW: 400, usableH: 300 }, pages: [{ id: "home", widgets }] }],
  });
  const button: WireWidget = { id: "w", type: "WIDGET_TYPE_BUTTON", col: 0, row: 0 };

  // Something below: it grows sideways instead.
  const below = widgetIn(setWidgetKind(page(button, { id: "b", col: 0, row: 1 }), "w", "slider"));
  assert.deepEqual([below.w ?? 1, below.h ?? 1], [2, 1]);

  // Boxed in on both sides: it stays one cell.
  const boxed = widgetIn(
    setWidgetKind(page(button, { id: "b", col: 0, row: 1 }, { id: "r", col: 1, row: 0 }), "w", "slider"),
  );
  assert.equal(boxed.type, "WIDGET_TYPE_SLIDER");
  assert.deepEqual([boxed.w ?? 1, boxed.h ?? 1], [1, 1]);
});

test("a hold has a press and a release, and replaces the tap", () => {
  const start = configWith({
    id: "w",
    col: 0,
    row: 0,
    flows: [
      { trigger: { gesture: "GESTURE_SHORT_PRESS" }, blocks: [] },
      { trigger: { gesture: "GESTURE_LONG_PRESS" }, blocks: [] },
    ],
  });
  const ptt = setHoldSteps(
    start,
    "w",
    { plugin: "discord", action: "ptt_start", args: {} },
    { plugin: "discord", action: "ptt_end", args: {} },
  );
  const widget = widgetIn(ptt);
  assert.deepEqual(
    widget.flows?.map((flow) => flow.trigger?.gesture),
    ["GESTURE_HOLD_START", "GESTURE_HOLD_END", "GESTURE_LONG_PRESS"],
    "the tap is gone, the long press stays",
  );
  assert.equal(holdStepsOf(widget).end?.action, "ptt_end");
  const none = widgetIn(setHoldSteps(ptt, "w", null, null));
  assert.deepEqual(none.flows?.map((flow) => flow.trigger?.gesture), ["GESTURE_LONG_PRESS"]);
});

test("an icon can follow a value, and clearing it removes it", () => {
  const start = configWith({ id: "w", col: 0, row: 0, props: { label: "Now playing" } });
  const art = setIconTemplate(start, "w", " {{media.current.art}} ");
  assert.equal(widgetIn(art).props?.iconTemplate, "{{media.current.art}}");
  assert.equal(setIconTemplate(art, "w", "{{media.current.art}}"), art, "unchanged spends no revision");
  assert.equal(widgetIn(setIconTemplate(art, "w", "")).props?.iconTemplate, undefined);
});

const masterVolume: ActionDefinition = {
  plugin: "system",
  pluginName: "System",
  action: "set_volume",
  description: "Set the output volume",
  arguments: [{ name: "percent", description: "", example: "50", kind: "number" }],
  slider: { argument: "percent", min: 0, max: 100, step: 1, feedback: "system.audio.volume", mute: { action: "toggle_mute", feedback: "system.audio.muted" } },
};

const mutableApp: ActionDefinition = {
  ...appVolume,
  pluginName: "System",
  slider: { ...appVolume.slider!, mute: { action: "toggle_app_mute", feedback: "system.audio.app.{{app}}.muted" } },
};

const toggleAppMute: ActionDefinition = {
  plugin: "system",
  action: "toggle_app_mute",
  description: "",
  arguments: [{ name: "app", description: "", example: "spotify.exe", kind: "text" }],
};

const mixer = [masterVolume, mutableApp, toggleAppMute, userVolume];

test("channels are each target with nothing to choose, and one per listed choice", () => {
  const channels = sliderChannels(sliderTargets(mixer), {
    "system:set_app_volume": {
      arguments: { app: [{ value: "obs64.exe", label: "OBS Studio" }, { value: "zen.exe" }] },
    },
  });
  assert.deepEqual(
    channels.map((channel) => [channel.group, channel.label]),
    [
      ["System", "Set the output volume"],
      ["System — Set one program's volume", "OBS Studio"],
      ["System — Set one program's volume", "zen.exe"],
    ],
    "Discord's users were not listed, so it offers none",
  );
  assert.deepEqual(channels[1]!.step, { plugin: "system", action: "set_app_volume", args: { app: "obs64.exe" } });
});

test("picking a channel names a fresh slider, wires its mute, and is read back", () => {
  const start = setWidgetKind(configWith({ id: "w", col: 0, row: 0, props: { label: "New button" } }), "w", "slider");
  const choices = { "system:set_app_volume": { arguments: { app: [{ value: "obs64.exe", label: "OBS Studio" }] } } };
  const [master, obs] = sliderChannels(sliderTargets(mixer), choices);
  const picked = setSliderChannel(start, "w", obs!, mixer, null);
  const widget = widgetIn(picked);
  assert.equal(widget.props?.label, "OBS Studio");
  assert.equal(widget.props?.icon, "volume");
  assert.equal(channelKeyOf(widget), obs!.key);
  assert.deepEqual(widget.flows?.[1], {
    trigger: { gesture: "GESTURE_SHORT_PRESS" },
    blocks: [{ block: "doAction", plugin: "system", action: "toggle_app_mute", args: { app: { text: "obs64.exe" } } }],
  });
  assert.equal(widget.states?.variable?.variable, "system.audio.app.obs64.exe.muted");
  assert.equal(widget.states?.looks?.[1]?.icon, "mute");

  // Another channel replaces the name it gave, the mute, and the looks; a
  // channel with no mute toggle in the catalogue leaves none behind.
  const moved = widgetIn(setSliderChannel(picked, "w", master!, mixer, obs!));
  assert.equal(moved.props?.label, "Set the output volume");
  assert.equal(moved.states?.variable?.variable, undefined, "toggle_mute is not in this catalogue");
  assert.equal(moved.flows?.length, 1);

  // A name the person typed is theirs.
  const renamed = withLabel(picked, "Stream");
  assert.equal(widgetIn(setSliderChannel(renamed, "w", master!, mixer, obs!)).props?.label, "Stream");
});

function withLabel(config: WireConfig, label: string): WireConfig {
  const widget = widgetIn(config);
  return configWith({ ...widget, props: { ...widget.props, label } });
}
