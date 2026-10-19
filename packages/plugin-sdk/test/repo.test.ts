import assert from "node:assert/strict";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { ABI_VERSION } from "../src/index.ts";
import { buildRepository, versionOrder } from "../src/repo.ts";
import { unzipEntry, zip } from "../src/zip.ts";

const contract = fileURLToPath(new URL("../../../contract/fixtures/plugin/", import.meta.url));
const update = process.env.UPDATE_FIXTURES === "1";

function scratch(): string {
  return mkdtempSync(join(tmpdir(), "flexdeck-repo-"));
}

function fdpkg(id: string, version: string, body = "export default {};\n"): Buffer {
  const manifest = { abi: ABI_VERSION, id, name: id.toUpperCase(), version, runtime: "node", entry: "index.js", permissions: ["network"] };
  return zip([
    { name: "plugin.json", data: Buffer.from(JSON.stringify(manifest)) },
    { name: "index.js", data: Buffer.from(body) },
  ]);
}

test("plugin.json reads back out of a package", () => {
  const raw = unzipEntry(fdpkg("obs", "1.0.0"), "plugin.json");
  assert.equal(JSON.parse(String(raw)).id, "obs");
  assert.equal(unzipEntry(fdpkg("obs", "1.0.0"), "missing.json"), null);
});

test("the sample package's repository is the committed contract fixture", () => {
  const dir = scratch();
  copyFileSync(join(contract, "sample.fdpkg"), join(dir, "sample.fdpkg"));
  buildRepository(dir, { name: "flexdeck contract", now: new Date("2026-01-01T00:00:00Z") });
  const actual = readFileSync(join(dir, "repository.min.json"));
  const path = join(contract, "repository.min.json");
  if (update || !existsSync(path)) {
    writeFileSync(path, actual);
    return;
  }
  assert.deepEqual(actual, readFileSync(path), "repository.min.json drifted; rerun with UPDATE_FIXTURES=1 if intended");
});

test("only the highest version of each plugin is listed, with addresses relative to the index", () => {
  const dir = scratch();
  mkdirSync(join(dir, "packages"));
  writeFileSync(join(dir, "packages", "obs-0.9.0.fdpkg"), fdpkg("obs", "0.9.0"));
  writeFileSync(join(dir, "packages", "obs-0.10.0.fdpkg"), fdpkg("obs", "0.10.0"));
  mkdirSync(join(dir, "icons"));
  writeFileSync(join(dir, "icons", "obs.png"), "png");
  writeFileSync(join(dir, "icons", "obs-64.png"), "png");
  const index = buildRepository(join(dir, "packages"), { out: dir, icons: join(dir, "icons") });
  assert.equal(index.plugins.length, 1);
  assert.equal(index.plugins[0]!.version, "0.10.0");
  assert.equal(index.plugins[0]!.package.url, "packages/obs-0.10.0.fdpkg");
  assert.equal(index.plugins[0]!.icon, "icons/obs.png");
  assert.deepEqual(index.plugins[0]!.icons, { "64": "icons/obs-64.png" });
  assert.ok(existsSync(join(dir, "repository.json")));
});

test("a rebuild keeps the release date, and moves the update date only with the version", () => {
  const dir = scratch();
  writeFileSync(join(dir, "obs-1.0.0.fdpkg"), fdpkg("obs", "1.0.0"));
  buildRepository(dir, { now: new Date("2026-01-01T00:00:00Z") });
  let index = buildRepository(dir, { now: new Date("2026-02-01T00:00:00Z") });
  assert.equal(index.plugins[0]!.latestUpdateDate, "2026-01-01T00:00:00Z", "same version, same date");

  writeFileSync(join(dir, "obs-1.1.0.fdpkg"), fdpkg("obs", "1.1.0"));
  index = buildRepository(dir, { now: new Date("2026-03-01T00:00:00Z") });
  assert.equal(index.plugins[0]!.releaseDate, "2026-01-01T00:00:00Z");
  assert.equal(index.plugins[0]!.latestUpdateDate, "2026-03-01T00:00:00Z");
});

test("a published version that comes back with different bytes is refused", () => {
  const dir = scratch();
  writeFileSync(join(dir, "obs-1.0.0.fdpkg"), fdpkg("obs", "1.0.0", "a"));
  buildRepository(dir);
  writeFileSync(join(dir, "obs-1.0.0.fdpkg"), fdpkg("obs", "1.0.0", "b"));
  assert.throws(() => buildRepository(dir), /never changes/);
});

function iconPack(dir: string, version: string, icons = 2): Buffer {
  return zip([
    { name: `${dir}/manifest.json`, data: Buffer.from(JSON.stringify({ Name: "Essentials", Author: "FlexDeck", Version: version })) },
    { name: `${dir}/icons.json`, data: Buffer.from(JSON.stringify(Array.from({ length: icons }, (_, n) => ({ path: `${n}.svg` })))) },
  ]);
}

test("icon packs are listed beside the plugins, and left out when there are none", () => {
  const dir = scratch();
  mkdirSync(join(dir, "packages"));
  writeFileSync(join(dir, "packages", "obs-1.0.0.fdpkg"), fdpkg("obs", "1.0.0"));
  assert.equal(buildRepository(join(dir, "packages"), { out: dir }).iconPacks, undefined);
  mkdirSync(join(dir, "packs"));
  writeFileSync(join(dir, "packs", "essentials.zip"), iconPack("essentials", "2.0.0", 3));
  const index = buildRepository(join(dir, "packages"), { out: dir, iconPacks: join(dir, "packs") });
  assert.equal(index.plugins.length, 1);
  assert.deepEqual(
    { ...index.iconPacks![0]!, package: { ...index.iconPacks![0]!.package, sha256: "" } },
    { id: "essentials", name: "Essentials", version: "2.0.0", description: "", author: "FlexDeck", count: 3, package: { url: "packs/essentials.zip", sha256: "", size: index.iconPacks![0]!.package.size } },
  );
  writeFileSync(join(dir, "packs", "essentials.zip"), iconPack("essentials", "2.0.0", 4));
  assert.throws(() => buildRepository(join(dir, "packages"), { out: dir, iconPacks: join(dir, "packs") }), /different bytes/);
  writeFileSync(join(dir, "packs", "essentials.zip"), zip([{ name: "readme.txt", data: Buffer.from("x") }]));
  assert.throws(() => buildRepository(join(dir, "packages"), { out: dir, iconPacks: join(dir, "packs") }), /no manifest\.json/);
});

test("versions order numerically, like the agent", () => {
  assert.ok(versionOrder("1.10.0", "1.9.0") > 0);
  assert.ok(versionOrder("1.0.0-beta", "1.0.0") < 0);
  assert.equal(versionOrder("2.0.0", "2.0.0"), 0);
});
