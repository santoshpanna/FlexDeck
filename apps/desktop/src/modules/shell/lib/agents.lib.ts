import type { DeviceRow } from "./types.ts";

export interface AgentSnippet {
  id: "claude-code" | "cursor" | "vscode";
  label: string;
  text: string;
}

export function agentSnippets({ url, token }: { url: string; token: string }): AgentSnippet[] {
  const headers = { Authorization: `Bearer ${token}` };
  return [
    {
      id: "claude-code",
      label: "Claude Code",
      text: `claude mcp add --transport http --scope user flexdeck ${url} --header "Authorization: Bearer ${token}"`,
    },
    {
      id: "cursor",
      label: "Cursor",
      text: JSON.stringify({ mcpServers: { flexdeck: { url, headers } } }, null, 2),
    },
    {
      id: "vscode",
      label: "VS Code",
      text: JSON.stringify({ servers: { flexdeck: { type: "http", url, headers } } }, null, 2),
    },
  ];
}

export function splitAgents(devices: readonly DeviceRow[]): { devices: DeviceRow[]; agents: DeviceRow[] } {
  return {
    devices: devices.filter((device) => device.kind !== "agent"),
    agents: devices.filter((device) => device.kind === "agent").sort((a, b) => b.created_at - a.created_at),
  };
}
