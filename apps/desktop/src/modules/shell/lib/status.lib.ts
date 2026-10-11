import type { Status, TunnelState } from "./types.ts";

export type Tone = "ok" | "warn" | "off";

export interface StatusRow {
  label: string;
  detail: string;
  tone: Tone;
}

/**
 * Turn the agent's state into rows a person can read.
 *
 * Deliberately several rows rather than one light. "The server is up", "web
 * access is switched on" and "the portal can actually be reached" are separate
 * facts: the agent's own code keeps *enabled* and *running* apart on purpose,
 * calling a single combined indicator a comforting lie. Folding them back
 * together in the one place the user actually looks would undo that.
 */
export function statusRows(status: Status): StatusRow[] {
  const rows: StatusRow[] = [serverRow(status)];

  rows.push(webAccessRow(status));

  for (const warning of status.warnings) {
    rows.push({ label: "Needs attention", detail: warning, tone: "warn" });
  }

  return rows;
}

function serverRow(status: Status): StatusRow {
  if (status.server.error !== null) {
    // The window reads the agent in-process, so everything else here can be
    // perfectly healthy while nothing outside this machine can connect at all.
    // This is the one row that can report that, and without it a taken port
    // would look exactly like a working setup.
    return {
      label: "This computer",
      detail: status.server.error,
      tone: "warn",
    };
  }

  return {
    label: "This computer",
    detail: `Serving on https://${status.host}:${status.port}`,
    tone: "ok",
  };
}

function webAccessRow(status: Status): StatusRow {
  const { tunnel } = status;
  if (tunnel.portal === null) {
    return {
      label: "Web access",
      detail: "No portal address set. Add one under Web access.",
      tone: "off",
    };
  }

  if (tunnel.refused !== null) {
    // The portal is answering — it is REFUSING. Reading this as "could not be
    // reached" would send the user to check their network for a problem that is
    // not there, and reading it as "connected" is the comforting lie this panel
    // exists to avoid.
    return {
      label: "Web access",
      detail: `The portal refused this computer: ${tunnel.refused.message}`,
      tone: "warn",
    };
  }

  if (tunnel.enabled && tunnel.running) {
    return {
      label: "Web access",
      detail: `Connected to ${tunnel.portal}`,
      tone: "ok",
    };
  }

  if (tunnel.enabled) {
    return {
      label: "Web access",
      detail: `Switched on, but ${tunnel.portal} could not be reached.`,
      tone: "warn",
    };
  }

  return {
    label: "Web access",
    detail: "Off. Turn it on to control this computer from the website.",
    tone: "off",
  };
}

/**
 * What to say on the Web access card.
 *
 * Only says the website can reach this computer when a tunnel is actually up —
 * "switched on" stays true while the portal is unreachable, and saying the
 * website can reach this machine then is the comforting lie the rows above exist
 * to avoid.
 */
export function webAccessSummary(tunnel: TunnelState): string {
  if (tunnel.portal === null) {
    return "Set the portal address this computer should reach, then switch web access on.";
  }

  if (tunnel.refused !== null) {
    return "The website refused this computer — this usually means the two are running different versions. Update this app, then try again.";
  }

  if (!tunnel.enabled) {
    return "Turn this on to reach this computer from a browser on another device. Then connect that browser with a code under Browsers.";
  }

  if (!tunnel.running) {
    return "Switched on, but the website cannot reach this computer right now.";
  }

  return "Connected. A browser you have connected can reach this computer from anywhere.";
}
