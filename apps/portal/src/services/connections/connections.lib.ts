/**
 * The desktops this browser can reach, and how.
 *
 * There are no accounts. A browser reaches a desktop because that desktop gave
 * it a token: automatically, when the portal is open on the same computer as
 * the desktop app (`local`), or after the person typed a code the desktop showed
 * (`relay`, through the cloud). Those tokens are kept here, in this browser, and
 * nowhere else — the cloud never sees a list of them.
 *
 * Pure functions over a `Storage`-shaped object, so they test under `node
 * --test` without a browser.
 */

/** How a desktop is reached. */
export type Via = "local" | "relay";

export interface Connection {
  /** The desktop's identity: the SHA-256 of its certificate. */
  fingerprint: string;
  /** The desktop's own label, as it last reported it. */
  name: string;
  /** The credential the desktop issued this browser. */
  token: string;
  via: Via;
}

/**
 * Where the desktop app on this computer listens for this page: one above
 * `DEFAULT_PORT` in crates/flexdeck-server/src/settings.rs (`loopback_port` in
 * apps/agent).
 */
export const LOCAL_AGENT = "http://127.0.0.1:23540";

const KEY = "flexdeck.connections";

/** The part of `Storage` this module needs. */
export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** Read the saved connections. Anything unreadable reads as none. */
export function loadConnections(storage: StorageLike | null): Connection[] {
  try {
    const raw = storage?.getItem(KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.filter(isConnection) : [];
  } catch {
    return [];
  }
}

export function saveConnections(storage: StorageLike | null, connections: Connection[]): void {
  try {
    storage?.setItem(KEY, JSON.stringify(connections));
  } catch {
    // Storage blocked or full: the connection works for this visit, and the
    // person connects again next time. Better than failing the page.
  }
}

/**
 * Add or replace the connection for `next.fingerprint` and `next.via`.
 *
 * Keyed by both, because the same desktop can be reached both ways — from the
 * same computer, and through the cloud — with different tokens.
 */
export function upsert(connections: Connection[], next: Connection): Connection[] {
  return [
    ...connections.filter(
      (existing) => !(existing.fingerprint === next.fingerprint && existing.via === next.via),
    ),
    next,
  ];
}

/** Drop every way of reaching `fingerprint`. */
export function forget(connections: Connection[], fingerprint: string): Connection[] {
  return connections.filter((existing) => existing.fingerprint !== fingerprint);
}

/** Drop one way of reaching `fingerprint` — say, a local token the desktop no longer honours. */
export function forgetVia(connections: Connection[], fingerprint: string, via: Via): Connection[] {
  return connections.filter((existing) => !(existing.fingerprint === fingerprint && existing.via === via));
}

/**
 * The best way to reach `fingerprint`: directly on this computer if possible,
 * through the cloud otherwise.
 */
export function route(
  connections: Connection[],
  fingerprint: string,
  localAvailable: boolean,
): Connection | null {
  const mine = connections.filter((connection) => connection.fingerprint === fingerprint);
  return (
    (localAvailable ? mine.find((connection) => connection.via === "local") : undefined) ??
    mine.find((connection) => connection.via === "relay") ??
    null
  );
}

function isConnection(value: unknown): value is Connection {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.fingerprint === "string" &&
    typeof candidate.name === "string" &&
    typeof candidate.token === "string" &&
    (candidate.via === "local" || candidate.via === "relay")
  );
}
