import assert from "node:assert/strict";
import { test } from "node:test";

import { agentSnippets, splitAgents } from "../src/modules/shell/lib/agents.lib.ts";
import type { DeviceRow } from "../src/modules/shell/lib/types.ts";

const target = { url: "http://127.0.0.1:23540/mcp", token: "tok-123" };

test("each coding tool gets a snippet carrying the address and the token", () => {
  const snippets = agentSnippets(target);
  assert.deepEqual(
    snippets.map((snippet) => snippet.label),
    ["Claude Code", "Cursor", "VS Code"],
  );
  assert.equal(
    snippets[0]!.text,
    'claude mcp add --transport http --scope user flexdeck http://127.0.0.1:23540/mcp --header "Authorization: Bearer tok-123"',
  );
  assert.deepEqual(JSON.parse(snippets[1]!.text), {
    mcpServers: { flexdeck: { url: target.url, headers: { Authorization: "Bearer tok-123" } } },
  });
  assert.deepEqual(JSON.parse(snippets[2]!.text), {
    servers: { flexdeck: { type: "http", url: target.url, headers: { Authorization: "Bearer tok-123" } } },
  });
});

test("agent tokens are listed apart from tablets and browsers, newest first", () => {
  const row = (id: string, kind: DeviceRow["kind"], created_at: number): DeviceRow => ({ id, name: id, created_at, online: false, kind });
  const { devices, agents } = splitAgents([row("t", "tablet", 1), row("a", "agent", 2), row("b", "agent", 3), row("w", "browser-local", 4)]);
  assert.deepEqual(
    devices.map((device) => device.id),
    ["t", "w"],
  );
  assert.deepEqual(
    agents.map((device) => device.id),
    ["b", "a"],
  );
});
