import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { scaffold } from "../scaffold.mjs";

const template = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "plugins", "template");

test("a new plugin is the template, renamed, depending on the published sdk", () => {
  const dir = mkdtempSync(join(tmpdir(), "fd-create-"));
  try {
    const to = join(dir, "lights");
    scaffold({ from: template, to, id: "lights", name: "Desk lights", sdk: "^0.1.0" });
    const manifest = JSON.parse(readFileSync(join(to, "plugin.json"), "utf8"));
    assert.equal(manifest.id, "lights");
    assert.equal(manifest.name, "Desk lights");
    const pkg = JSON.parse(readFileSync(join(to, "package.json"), "utf8"));
    assert.equal(pkg.dependencies["@flexdeck/plugin-sdk"], "^0.1.0");
    assert.equal(pkg.name, "@flexdeck/plugin-lights");
    assert.match(readFileSync(join(to, "src", "index.ts"), "utf8"), /id: "lights"/);
    assert.throws(() => scaffold({ from: template, to, id: "lights" }), /already exists/);
    assert.throws(() => scaffold({ from: template, to: join(dir, "x"), id: "Bad Id" }), /lowercase/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
