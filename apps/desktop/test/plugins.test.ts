import assert from "node:assert/strict";
import { test } from "node:test";

import {
  entryAction,
  filterCatalog,
  filterCounts,
  formatSize,
  permissionLabel,
  pluginGroups,
  pluginState,
  relativeDate,
  sourceLabel,
} from "../src/modules/shell/lib/plugins.lib.ts";
import type { CatalogEntry, InstalledPlugin } from "../src/modules/shell/lib/types.ts";

function installed(overrides: Partial<InstalledPlugin> = {}): InstalledPlugin {
  return {
    id: "obs",
    name: "OBS Studio",
    version: "0.1.0",
    sha256: "",
    permissions: ["network"],
    enabled: true,
    source: { kind: "repository", url: "https://a.example/repository.min.json", name: "flexdeck" },
    installedAt: 0,
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

test("built-in rows are described from what the agent reported", () => {
  const groups = pluginGroups([{ id: "core", plugin: "Core", version: "1", actions: 5, status: null }]);
  assert.deepEqual(groups, [{ name: "Core", actions: 5, detail: "5 actions" }]);
  assert.equal(pluginGroups([{ id: "x", plugin: "X", version: "1", actions: 1, status: null }])[0]?.detail, "1 action");
});

test("no plugins is an empty list, not a placeholder row", () => {
  assert.deepEqual(pluginGroups([]), []);
});

test("an installed plugin's state names the reason, in order of what matters", () => {
  assert.deepEqual(pluginState(installed()), { word: "Running", tone: "ok", reason: null });
  assert.equal(pluginState(installed({ enabled: false, error: "x" })).word, "Off", "off wins: it was asked for");
  assert.deepEqual(pluginState(installed({ error: "ABI 1" })), { word: "Failed", tone: "warn", reason: "ABI 1" });
  assert.equal(pluginState(installed({ running: false, status: "restarting" })).reason, "restarting");
  assert.equal(pluginState(installed({ status: "OBS refused the password" })).word, "Trouble");
});

test("permissions are said in words a person can weigh", () => {
  assert.match(permissionLabel("network"), /network/);
  assert.equal(permissionLabel("something-new"), "something-new", "an unknown one is shown, not hidden");
});

test("an entry's button says what pressing it does", () => {
  assert.deepEqual(entryAction(entry()), { label: "Install", enabled: true, primary: true });
  assert.equal(entryAction(entry({ installedVersion: "0.2.0", installedFromHere: true })).label, "Installed");
  assert.equal(entryAction(entry({ installedVersion: "0.2.0", installedFromHere: true })).enabled, false);
  assert.equal(entryAction(entry({ installedVersion: "0.1.0", installedFromHere: true, update: true })).label, "Update");
  assert.equal(
    entryAction(entry({ installedVersion: "0.2.0", installedFromHere: false })).label,
    "Replace",
    "the same id from another repository replaces what is there",
  );
});

test("search and chips filter what is loaded", () => {
  const all = [
    entry(),
    entry({ id: "lights", name: "Hue lights", author: "someone", repo: { url: "b", name: "B", kind: "vetted" } }),
    entry({ id: "mine", name: "Mine", repo: { url: "c", name: "C", kind: "custom" }, update: true }),
  ];
  assert.equal(filterCatalog(all, "", "all").length, 3);
  assert.deepEqual(filterCatalog(all, "HUE", "all").map((e) => e.id), ["lights"]);
  assert.deepEqual(filterCatalog(all, "someone", "all").map((e) => e.id), ["lights"], "by author");
  assert.deepEqual(filterCatalog(all, "", "custom").map((e) => e.id), ["mine"]);
  assert.deepEqual(filterCatalog(all, "", "updates").map((e) => e.id), ["mine"]);
  assert.deepEqual(filterCounts(all), { all: 3, official: 1, vetted: 1, custom: 1, updates: 1 });
});

test("dates read as how long ago", () => {
  const now = Date.parse("2026-10-07T12:00:00Z");
  assert.equal(relativeDate("2026-10-07T11:59:30Z", now), "just now");
  assert.equal(relativeDate("2026-10-07T11:55:00Z", now), "5 minutes ago");
  assert.equal(relativeDate("2026-10-06T12:00:00Z", now), "1 day ago");
  assert.equal(relativeDate("2026-09-20T12:00:00Z", now), "2 weeks ago");
  assert.equal(relativeDate("2024-10-01T12:00:00Z", now), "2 years ago");
  assert.equal(relativeDate(now / 1000 - 7200, now), "2 hours ago", "seconds since the epoch, as the agent counts");
  assert.equal(relativeDate(null, now), "");
  assert.equal(relativeDate("not a date", now), "");
});

test("sizes and sources read as words", () => {
  assert.equal(formatSize(512), "512 B");
  assert.equal(formatSize(20 * 1024), "20 KB");
  assert.equal(formatSize(3 * 1024 * 1024), "3.0 MB");
  assert.equal(sourceLabel(installed()), "from flexdeck");
  assert.equal(sourceLabel(installed({ source: { kind: "marketplace" } })), "from the marketplace");
});
