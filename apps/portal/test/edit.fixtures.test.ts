import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { EDIT_DIR, fixtures, render } from "../scripts/gen-edit.ts";

test("the editing answers the agent's MCP tools are held to are the editor's (run pnpm gen:edit)", () => {
  for (const [file, value] of Object.entries(fixtures())) {
    assert.equal(readFileSync(`${EDIT_DIR}${file}`, "utf8").replace(/\r\n/g, "\n"), render(value), file);
  }
});
