import assert from "node:assert/strict";
import { test } from "node:test";

import { parseSdkVersion } from "../release/sdk-version.ts";
import { parseVersion } from "../release/version.ts";
import { buildChangelog, buildSdkReleases, groupEntries, pendingSdkReleases, parseLine, parseMessage, releaseMarkdown, releaseNotes, releaseOf, releaseVersion, sdkReleasesOf, toMarkdown } from "./changelog.lib.ts";

test("a release's notes are its blurb and its changes", () => {
  const [release] = buildChangelog([{ hash: "a1", date: "2026-10-20", message: "First\nrelease: v0.1.0\nThe deck.\nfeat(android): faders" }]).releases;
  assert.equal(releaseMarkdown(release), "The deck.\n\n### Added\n\n- **android** Faders.\n");
});

test("a tag becomes a version name and an ever-growing Android versionCode", () => {
  assert.deepEqual(parseVersion("v0.1.0"), { name: "0.1.0", code: 100 });
  assert.deepEqual(parseVersion("v1.2.3"), { name: "1.2.3", code: 10203 });
  assert.deepEqual(parseVersion("v1.0.0-rc.1"), { name: "1.0.0-rc.1", code: 10000 });
  assert.throws(() => parseVersion("v0.100.0"), /under 100/);
  assert.throws(() => parseVersion("latest"), /not a release version/);
});

const commit = (hash: string, message: string, date = "2026-10-20") => ({ hash, date, message });

test("a release line marks the commit that cuts a release", () => {
  assert.equal(releaseVersion("First public release\nrelease: v0.1.0\nfeat(desktop): pairing"), "v0.1.0");
  assert.equal(releaseVersion("Ship fills\nfeat(ui): fills"), null);
  assert.equal(releaseVersion("Oops\nrelease: 0.1"), null);
});

test("a release line's scope picks the apps it ships", () => {
  assert.deepEqual(releaseOf("Ship\nrelease: v0.1.0"), { version: "v0.1.0", platforms: ["desktop", "android", "server"] });
  assert.deepEqual(releaseOf("Ship\nrelease(server): v0.1.1"), { version: "v0.1.1", platforms: ["server"] });
  assert.deepEqual(releaseOf("Ship\nrelease(android): v0.1.1"), { version: "v0.1.1", platforms: ["android"] });
  assert.deepEqual(releaseOf("Ship\nrelease(Android, desktop): v0.1.2"), { version: "v0.1.2", platforms: ["desktop", "android"] });
  assert.throws(() => releaseOf("Ship\nrelease(ios): v0.1.3"), /unknown release target "ios"/);
  assert.deepEqual(releaseNotes("Ship\nrelease(desktop): v0.1.1\nFaster."), { title: "Ship", blurb: "Faster." });
});

test("an SDK release line publishes that SDK and is not an app release", () => {
  assert.deepEqual(sdkReleasesOf("Ship the SDK\nrelease(js-sdk): v0.2.0"), [{ sdk: "js-sdk", version: "v0.2.0" }]);
  assert.deepEqual(sdkReleasesOf("Ship\nrelease(rust-sdk, python-sdk): v0.3.0-rc.1"), [
    { sdk: "rust-sdk", version: "v0.3.0-rc.1" },
    { sdk: "python-sdk", version: "v0.3.0-rc.1" },
  ]);
  assert.equal(releaseOf("Ship the SDK\nrelease(js-sdk): v0.2.0"), null);
  assert.deepEqual(sdkReleasesOf("Ship\nrelease: v0.2.0"), []);
  assert.throws(() => releaseOf("Ship\nrelease(desktop, js-sdk): v0.2.0"), /names apps and SDKs/);
});

test("an app release and an SDK release can share a commit", () => {
  const message = "Ship\nrelease(desktop): v0.4.0\nrelease(js-sdk): v0.2.0";
  assert.deepEqual(releaseOf(message), { version: "v0.4.0", platforms: ["desktop"] });
  assert.deepEqual(sdkReleasesOf(message), [{ sdk: "js-sdk", version: "v0.2.0" }]);
});

test("an SDK release neither cuts the app changelog nor counts against its versions", () => {
  const log = buildChangelog([
    commit("a1", "First\nrelease: v0.1.0\nfeat(desktop): pairing"),
    commit("b2", "SDK\nrelease(js-sdk): v0.1.0\nfeat(sdk): definePlugin"),
  ]);
  assert.deepEqual(log.releases.map((release) => release.version), ["v0.1.0"]);
  assert.equal(log.next.length, 1);
});

test("CI publishes the newest untagged release of each SDK package", () => {
  const releases = buildSdkReleases([
    commit("a1", "SDK\nrelease(js-sdk): v0.1.0"),
    commit("b2", "SDK\nrelease(js-sdk): v0.2.0"),
    commit("c3", "SDK\nrelease(rust-abi): v0.1.0"),
    commit("d4", "SDK\nrelease(python-sdk): v0.1.0"),
  ]);
  assert.deepEqual(pendingSdkReleases(releases, new Set(["js-sdk-v0.1.0", "rust-abi-v0.1.0"])), {
    base: [
      { sdk: "js-sdk", version: "v0.2.0", hash: "b2" },
      { sdk: "python-sdk", version: "v0.1.0", hash: "d4" },
    ],
    dependents: [],
  });
  assert.throws(() => buildSdkReleases([commit("a1", "x\nrelease(js-sdk): v0.1.0"), commit("b2", "y\nrelease(js-sdk): v0.1.0")]), /released twice/);
});

test("a package that depends on another builds against its newest release, in the wave after", () => {
  const releases = buildSdkReleases([
    commit("a1", "SDK\nrelease(js-sdk): v0.1.0"),
    commit("b2", "SDK\nrelease(rust-abi): v0.1.0"),
    commit("c3", "SDK\nrelease(create-plugin): v0.4.0"),
    commit("d4", "SDK\nrelease(rust-sdk, rust-abi): v0.2.0"),
    commit("e5", "SDK\nrelease(js-sdk): v0.3.0"),
  ]);
  assert.deepEqual(pendingSdkReleases(releases, new Set(["js-sdk-v0.1.0", "rust-abi-v0.1.0"])), {
    base: [
      { sdk: "js-sdk", version: "v0.3.0", hash: "e5" },
      { sdk: "rust-abi", version: "v0.2.0", hash: "d4" },
    ],
    dependents: [
      // As of its own commit: the js-sdk release after it doesn't count.
      { sdk: "create-plugin", version: "v0.4.0", hash: "c3", requires: "v0.1.0" },
      // A dependency released in the same commit does.
      { sdk: "rust-sdk", version: "v0.2.0", hash: "d4", requires: "v0.2.0" },
    ],
  });
});

test("a dependent released before its dependency has nothing to build against", () => {
  assert.deepEqual(buildSdkReleases([commit("a1", "SDK\nrelease(rust-sdk): v0.1.0")]), [{ sdk: "rust-sdk", version: "v0.1.0", hash: "a1" }]);
});

test("an SDK version may be a pre-release", () => {
  assert.deepEqual(parseSdkVersion("v0.2.0"), { name: "0.2.0", prerelease: false });
  assert.deepEqual(parseSdkVersion("v0.2.0-rc.1"), { name: "0.2.0-rc.1", prerelease: true });
  assert.throws(() => parseSdkVersion("0.2"), /not a release version/);
});

test("release notes are the subject and the untyped prose, without trailers", () => {
  const notes = releaseNotes("First public release\nrelease: v0.1.0\nThe tablet in your drawer.\nfeat(desktop): pairing\nOpen source.\n\nCo-Authored-By: A <a@b.c>");
  assert.deepEqual(notes, { title: "First public release", blurb: "The tablet in your drawer. Open source." });
});

test("a release holds every change since the previous release, its own included", () => {
  const log = buildChangelog([
    commit("a1", "Early work\nfeat(ui): fills"),
    commit("b2", "Preview\nrelease: v0.0.1\nfix(ui): popover"),
    commit("c3", "Pairing\nfeat(desktop): pairing"),
    commit("d4", "First public release\nrelease: v0.1.0\nThe deck.\nfeat(android): faders"),
    commit("e5", "After\nfeat(android): play listing"),
  ]);
  assert.deepEqual(
    log.releases.map((release) => [release.version, release.hash, release.title, release.blurb, release.groups.flatMap((group) => group.items.map((item) => item.text))]),
    [
      ["v0.1.0", "d4", "First public release", "The deck.", ["Pairing.", "Faders."]],
      ["v0.0.1", "b2", "Preview", "", ["Fills.", "Popover."]],
    ],
  );
  assert.deepEqual(log.next[0].items, [{ area: "ANDROID", text: "Play listing." }]);
});

test("releasing the same version twice is an error", () => {
  assert.throws(() => buildChangelog([commit("a1", "One\nrelease: v0.1.0"), commit("b2", "Two\nrelease: v0.1.0")]), /released twice/);
});

test("a feat line becomes an Added entry in its area", () => {
  assert.deepEqual(parseLine("feat(android): add connection panel"), { group: "Added", entry: { area: "ANDROID", text: "Add connection panel." } });
});

test("internal types never reach the changelog", () => {
  for (const line of ["chore: license core", "ci: run on dispatch", "test(portal): cover fills", "docs: add guides", "refactor(ui): split"]) {
    assert.equal(parseLine(line), null, line);
  }
});

test("internal scopes are dropped even on a feat", () => {
  assert.equal(parseLine("feat(proto): add PageShown"), null);
});

test("a plugin scope names the plugin on the line", () => {
  assert.equal(parseLine("feat(obs): bump to 0.4 with scene presets")?.entry.text, "OBS Studio: bump to 0.4 with scene presets.");
  assert.equal(parseLine("fix(streamerbot): reconnect after restart")?.entry.area, "PLUGINS");
});

test("an unknown scope lands in GENERAL and is reported", () => {
  assert.deepEqual(parseLine("fix(widgets): stop flicker"), { group: "Fixed", entry: { area: "GENERAL", text: "Stop flicker." }, unknownScope: "widgets" });
});

test("a scope-less line is GENERAL without a warning", () => {
  assert.deepEqual(parseLine("perf: faster startup"), { group: "Improved", entry: { area: "GENERAL", text: "Faster startup." } });
});

test("plain subjects and trailers are ignored", () => {
  const lines = parseMessage("Ship live config feed\nfeat(portal): ship FillPicker\nci: dispatch only\n\nCo-Authored-By: Someone <x@y.z>");
  assert.deepEqual(
    lines.map((line) => line.entry.text),
    ["Ship FillPicker."],
  );
});

test("groups come out in a fixed order with duplicates removed", () => {
  const groups = groupEntries(parseMessage("fix(ui): a\nfeat(ui): b\nfeat(ui): b\nsec(desktop): c"));
  assert.deepEqual(
    groups.map((group) => [group.name, group.items.length]),
    [
      ["Added", 1],
      ["Fixed", 1],
      ["Security", 1],
    ],
  );
});

test("markdown carries next and releases", () => {
  const md = toMarkdown({
    next: groupEntries(parseMessage("feat(android): play listing")),
    releases: [{ version: "v0.1.0", hash: "d4", date: "2026-10-20", title: "First public release", blurb: "", groups: groupEntries(parseMessage("feat(desktop): pairing")) }],
  });
  assert.match(md, /## Next\n\n### Added\n\n- \*\*android\*\* Play listing\./);
  assert.match(md, /## v0\.1\.0 — 2026-10-20\n\n\*\*First public release\*\*/);
});
