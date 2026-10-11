import type { DeviceRow, Status, TunnelState } from "./types.ts";

/** What the status plate says about one part of the system. */
export interface Readout {
  label: string;
  value: string;
  led: "ok" | "wait" | "warn" | "off";
}

/**
 * The three lights on the window's faceplate: is this computer serving, is a
 * tablet here, can the website reach it. Each keeps its own fact — folding them
 * into one "all good" light is the comforting lie `statusRows` warns about.
 */
export function readouts(status: Status): Readout[] {
  const tablets = status.devices.filter((device) => device.kind === "tablet");
  const online = tablets.filter((device) => device.online).length;
  return [
    {
      label: "Agent",
      value: status.server.error === null ? "Online" : "Blocked",
      led: status.server.error === null ? "ok" : "warn",
    },
    {
      label: "Tablets",
      value: tablets.length === 0 ? "None" : `${online}/${tablets.length} live`,
      led: online > 0 ? "ok" : tablets.length > 0 ? "wait" : "off",
    },
    { label: "Web", ...web(status.tunnel) },
  ];
}

function web(tunnel: TunnelState): Pick<Readout, "value" | "led"> {
  if (tunnel.refused !== null) return { value: "Refused", led: "warn" };
  if (tunnel.enabled && tunnel.running) return { value: "On", led: "ok" };
  if (tunnel.enabled) return { value: "Trying", led: "wait" };
  return { value: "Off", led: "off" };
}

export type NextStep =
  | { kind: "blocked"; detail: string }
  | { kind: "pair" }
  | { kind: "wake"; tablet: string }
  | { kind: "build"; editor: string | null };

/**
 * The ONE thing the window suggests doing next. The window used to list every
 * card with equal weight; this is what replaces that — a single plate that
 * changes as setup progresses.
 */
export function nextStep(status: Status): NextStep {
  if (status.server.error !== null) return { kind: "blocked", detail: status.server.error };
  const tablets = status.devices.filter((device) => device.kind === "tablet");
  if (tablets.length === 0) return { kind: "pair" };
  if (!tablets.some((device) => device.online)) {
    const newest = [...tablets].sort((a, b) => b.created_at - a.created_at)[0] as DeviceRow;
    return { kind: "wake", tablet: newest.name };
  }
  return { kind: "build", editor: editorUrl(status.tunnel.portal) };
}

/**
 * The editor's address, from the portal's tunnel address: the same host over
 * HTTP(S), at `/config`. `null` when there is no portal, or the address is not
 * one this can read — better no link than a wrong one.
 */
export function editorUrl(portal: string | null): string | null {
  if (portal === null || portal.trim() === "") return null;
  try {
    const url = new URL(portal.trim());
    const scheme = url.protocol === "wss:" ? "https:" : url.protocol === "ws:" ? "http:" : url.protocol;
    if (scheme !== "https:" && scheme !== "http:") return null;
    return `${scheme}//${url.host}/config`;
  } catch {
    return null;
  }
}

/**
 * The tablet that appeared since `before` was taken, if one did. How the Pair
 * screen knows the code it is showing was just used.
 */
export function newTablet(before: readonly string[], devices: readonly DeviceRow[]): DeviceRow | null {
  return devices.find((device) => device.kind === "tablet" && !before.includes(device.id)) ?? null;
}
