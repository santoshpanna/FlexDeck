import assert from "node:assert/strict";
import { test } from "node:test";

import { describeTrigger } from "../src/modules/config/components/AutomationEditor/describe.lib.ts";
import { groupActions, pluginLabel } from "../src/modules/config/lib/actionPicker.lib.ts";
import { pressActionKey, setWidgetBinding, stateVariableOf } from "../src/modules/config/lib/editing.lib.ts";
import {
  checkPackageFile,
  entryAction,
  filterCatalog,
  filterCounts,
  filterIconPacks,
  iconPackCounts,
  permissionLabel,
  pluginState,
  relativeDate,
} from "../src/modules/config/lib/plugins.lib.ts";
import type { CatalogEntry, InstalledPlugin, RepoKind, RepositoryIconPack } from "../src/services/api/api.ts";

function installed(overrides: Partial<InstalledPlugin> = {}): InstalledPlugin {
  return {
    id: "obs",
    name: "OBS Studio",
    version: "0.1.0",
    permissions: ["network"],
    enabled: true,
    source: { kind: "marketplace" },
    running: true,
    status: null,
    error: null,
    ...overrides,
  };
}

function entry(overrides: Partial<CatalogEntry> = {}): CatalogEntry {
  return {
    id: "obs",
    name: "OBS Studio",
    version: "0.2.0",
    description: "Control OBS",
    author: "flexdeck",
    abi: 2,
    permissions: ["network"],
    package: { url: "obs.fdpkg", sha256: "", size: 0 },
    repo: { url: "https://a.example/repository.min.json", name: "flexdeck", kind: "official" },
    iconData: null,
    installedVersion: null,
    installedFromHere: false,
    update: false,
    ...overrides,
  };
}

test("an entry is Install, Update, Installed or Replace relative to the desktop", () => {
  assert.equal(entryAction(entry()).label, "Install");
  assert.equal(entryAction(entry({ installedVersion: "0.1.0", installedFromHere: true, update: true })).label, "Update");
  assert.equal(entryAction(entry({ installedVersion: "0.2.0", installedFromHere: true })).enabled, false);
  assert.equal(entryAction(entry({ installedVersion: "0.2.0" })).label, "Replace");
});

test("the marketplace filters by search and by repository kind", () => {
  const all = [entry(), entry({ id: "mine", name: "Mine", repo: { url: "c", name: "C", kind: "custom" }, update: true })];
  assert.deepEqual(filterCatalog(all, "mine", "all").map((e) => e.id), ["mine"]);
  assert.deepEqual(filterCatalog(all, "", "official").map((e) => e.id), ["obs"]);
  assert.deepEqual(filterCatalog(all, "", "updates").map((e) => e.id), ["mine"]);
  assert.equal(filterCounts(all).custom, 1);
});

test("icon packs filter by search and repository kind, and never show under updates", () => {
  const pack = (id: string, kind: RepoKind, description?: string): RepositoryIconPack => ({
    id,
    name: id,
    version: "1.0.0",
    description,
    author: "flexdeck",
    count: 3,
    installed: false,
    repo: { url: kind, name: kind, kind },
  });
  const all = [pack("essentials", "official", "Everyday glyphs"), pack("neon", "custom")];
  assert.deepEqual(filterIconPacks(all, "glyphs", "all").map((p) => p.id), ["essentials"]);
  assert.deepEqual(filterIconPacks(all, "", "custom").map((p) => p.id), ["neon"]);
  assert.deepEqual(filterIconPacks(all, "", "updates"), []);
  const counts = iconPackCounts(all);
  assert.deepEqual([counts.all, counts.official, counts.custom, counts.updates], [2, 1, 1, 0]);
});

test("a dropped file is checked before it is read or sent", () => {
  assert.equal(checkPackageFile("weather-0.1.0.fdpkg", 2048, 64 * 1024 * 1024, false), null);
  assert.equal(checkPackageFile("WEATHER.FDPKG", 2048, 64 * 1024 * 1024, false), null, "any case");
  assert.match(checkPackageFile("weather.zip", 2048, 1e9, false) ?? "", /end in \.fdpkg/);
  assert.match(checkPackageFile("empty.fdpkg", 0, 1e9, false) ?? "", /empty/);
  assert.match(checkPackageFile("big.fdpkg", 2_000_000, 740 * 1024, true) ?? "", /through the cloud.*desktop window/);
  assert.match(checkPackageFile("big.fdpkg", 2_000_000, 1024, false) ?? "", /the limit is 1 KB/);
});

test("dates read as how long ago", () => {
  const now = Date.parse("2026-10-07T12:00:00Z");
  assert.equal(relativeDate("2026-10-04T12:00:00Z", now), "3 days ago");
  assert.equal(relativeDate(undefined, now), "");
});

test("an installed plugin's state carries its reason", () => {
  assert.equal(pluginState(installed()).word, "Running");
  assert.deepEqual(pluginState(installed({ status: "OBS is not running" })), {
    word: "Trouble",
    tone: "warn",
    reason: "OBS is not running",
  });
  assert.equal(pluginState(installed({ enabled: false })).tone, "off");
});

test("permissions read as what the plugin may do", () => {
  assert.match(permissionLabel("processes"), /Start other programs/);
});

test("the new triggers describe themselves in a line", () => {
  assert.equal(
    describeTrigger({ on: "pluginEvent", plugin: "obs", event: "scene_changed", filter: { scene: "Live" } }),
    "on obs:scene_changed where scene=Live",
  );
  assert.equal(describeTrigger({ on: "schedule", everySeconds: 300 }), "every 5 minutes");
  assert.equal(describeTrigger({ on: "schedule", everySeconds: 3600 }), "every hour");
  assert.equal(describeTrigger({ on: "schedule", at: "09:30", days: ["mon", "fri"] }), "at 09:30 on mon, fri");
  assert.equal(describeTrigger({ on: "schedule", at: "09:30" }), "at 09:30 every day");
});

test("a widget is bound to live values and unbound by clearing them", () => {
  const config = {
    schema: 2,
    profiles: [{ id: "p", pages: [{ id: "page", widgets: [{ id: "rec", props: { label: "Record" } }] }] }],
  };
  const bound = setWidgetBinding(config, "rec", {
    stateVariable: " obs.recording ",
    labelTemplate: "REC {{obs.record_time}}",
  });
  const widget = bound.profiles?.[0]?.pages?.[0]?.widgets?.[0];
  assert.deepEqual(widget?.props, { label: "Record", labelTemplate: "REC {{obs.record_time}}" });
  // "Light up while" is two states, picked by the variable.
  assert.deepEqual(widget?.states?.variable, {
    variable: "obs.recording",
    cases: [{ value: "true", state: "on" }],
    otherwise: "off",
  });
  assert.equal(stateVariableOf(widget!), "obs.recording");

  const unbound = setWidgetBinding(bound, "rec", { stateVariable: "", labelTemplate: "  " });
  const after = unbound.profiles?.[0]?.pages?.[0]?.widgets?.[0];
  assert.deepEqual(after?.props, { label: "Record", noAutoLight: true }, "a light cleared by hand stays cleared");
  assert.equal(after?.states, undefined);

  // States this editor did not make are not its to remove.
  const rich = { id: "w", states: { looks: [{ id: "a" }, { id: "b" }, { id: "c" }], cycle: {} } };
  assert.equal(stateVariableOf(rich), "");
  const kept = setWidgetBinding({ schema: 2, profiles: [{ id: "p", pages: [{ id: "page", widgets: [rich] }] }] }, "w", {
    stateVariable: "",
    labelTemplate: "",
  });
  assert.deepEqual(kept.profiles?.[0]?.pages?.[0]?.widgets?.[0]?.states, rich.states);
});


test("the action picker groups by plugin name and finds by every word", () => {
  const action = (plugin: string, pluginName: string | undefined, name: string, description: string) => ({
    plugin,
    pluginName,
    action: name,
    description,
    arguments: [],
  });
  const catalogue = [
    action("media", "Media", "volume_up", "Volume up one step"),
    action("obs", "OBS Studio", "start_recording", "Start recording"),
    action("obs", "OBS Studio", "set_scene", "Switch the live scene"),
    action("core", undefined, "hotkey", "Press a keyboard shortcut"),
  ];
  const all = groupActions(catalogue, "");
  assert.deepEqual(all.map((group) => group.name), ["Media", "OBS Studio", "core"], "agent order, real names");
  assert.equal(all[1]?.actions.length, 2);
  assert.deepEqual(groupActions(catalogue, "obs rec").flatMap((g) => g.actions.map((a) => a.action)), ["start_recording"]);
  assert.deepEqual(groupActions(catalogue, "STUDIO scene").flatMap((g) => g.actions.map((a) => a.action)), ["set_scene"]);
  assert.deepEqual(groupActions(catalogue, "nothing like it"), []);
  assert.equal(pluginLabel({ plugin: "core", pluginName: "  " }), "core");
});

test("a stored action finds its place in the picker", () => {
  const catalogue = [
    { plugin: "obs", action: "toggle_virtual_camera", description: "", arguments: [] },
    { plugin: "core", action: "notify", description: "", arguments: [] },
    { plugin: "notifications", action: "notify", description: "", arguments: [] },
  ];
  // Stored as two fields; the picker and the editor need one key.
  assert.equal(pressActionKey({ plugin: "obs", action: "toggle_virtual_camera" }, catalogue), "obs:toggle_virtual_camera");
  assert.equal(pressActionKey({ action: "toggle_virtual_camera" }, catalogue), "obs:toggle_virtual_camera", "older builds saved no plugin");
  assert.equal(pressActionKey({ action: "notify" }, catalogue), "notify", "ambiguous without a plugin: left alone");
  assert.equal(pressActionKey(null, catalogue), "");
});
