import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { conform, describe, findFlexdeck, passed } from "../src/conformance.ts";
import { pack } from "../src/pack.ts";

const template = fileURLToPath(new URL("../../../plugins/template/", import.meta.url));
const flexdeck = findFlexdeck(template);

test("FlexDeck is found from FLEXDECK_BIN first, and a missing one is no answer", () => {
  assert.equal(findFlexdeck(tmpdir(), { FLEXDECK_BIN: join(tmpdir(), "no-such-flexdeck") }), null);
});

test("the template passes FlexDeck's own verification", { skip: flexdeck ? false : "build the agent (cargo build -p flexdeck-agent) to run this" }, async () => {
  const out = mkdtempSync(join(tmpdir(), "fd-sdk-verify-"));
  try {
    const packed = await pack(template, { out: join(out, "template.fdpkg") });
    const report = conform(packed.file, { flexdeck: flexdeck! });
    assert.ok(passed(report), describe(report));
    assert.ok(report.checks.some((check) => check.name === "it stops when asked"));
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("a broken package is named broken, not passed", { skip: flexdeck ? false : "needs the agent" }, () => {
  const dir = mkdtempSync(join(tmpdir(), "fd-sdk-broken-"));
  try {
    writeFileSync(join(dir, "plugin.json"), JSON.stringify({ abi: 4, id: "Broken!", name: "x", version: "1", runtime: "node", entry: "a.js" }));
    const report = conform(dir, { flexdeck: flexdeck! });
    assert.equal(passed(report), false);
    assert.match(describe(report), /FAIL.*plugin.json/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
