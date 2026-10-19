import { QWebSocket, type WSAdapter, type WSMessageEvent } from "@questorylabs/qhttp/ws";

import type { Connection } from "../connections/connections.lib.ts";
import type { SourceStatus, WidgetPreview } from "./api.ts";

/**
 * What a desktop says happened, as it happens — the same messages its tablets
 * hear, in their proto3 JSON shape.
 */
export type LiveMessage =
  /** The socket is (re)open: anything read before it may be stale. */
  | { kind: "open" }
  /** The configuration is at `rev` now. */
  | { kind: "revision"; rev: number }
  /** A key's look, now. */
  | { kind: "state"; widgetId: string; state: NonNullable<WidgetPreview["state"]> }
  /** The page the tablets are on. Empty `pageId`: they moved by `step`. */
  | { kind: "page"; pageId: string; step: number }
  /** Devices were paired, revoked, connected or disconnected. */
  | { kind: "devices" }
  /** How a data source's last run went (`src.<id>`, or `src.<id>.@<widget>`). */
  | { kind: "source"; source: string; status: SourceStatus };

export type LiveListener = (message: LiveMessage) => void;

/** Why a desktop ended the socket, in its own words. */
interface Refusal {
  code: string;
  message: string;
}

/** The longest wait between reconnects. */
const MAX_BACKOFF_MS = 15_000;

/**
 * Read one frame. Unknown kinds are ignored, so a newer desktop never breaks
 * an older page.
 */
export function parseLive(text: string): LiveMessage | Refusal | null {
  let frame: Record<string, unknown>;
  try {
    frame = JSON.parse(text) as Record<string, unknown>;
  } catch {
    return null;
  }
  const body = (key: string) => frame[key] as Record<string, unknown> | undefined;
  // 64-bit numbers travel as strings in proto3 JSON.
  const revision = body("patch") ?? body("welcome");
  if (revision) return { kind: "revision", rev: Number(revision.rev ?? 0) };
  const state = body("state");
  if (state) {
    // `value` stays: it is where a slider's thumb sits.
    const { widgetId, ok: _ok, error: _error, ...look } = state;
    return { kind: "state", widgetId: String(widgetId ?? ""), state: look as NonNullable<WidgetPreview["state"]> };
  }
  const page = body("page");
  if (page) return { kind: "page", pageId: String(page.pageId ?? ""), step: Number(page.step ?? 0) };
  if (body("devices")) return { kind: "devices" };
  const source = body("source");
  if (source) {
    return {
      kind: "source",
      source: String(source.source ?? ""),
      status: { ok: source.ok === true, error: source.error ? String(source.error) : null, updated: Number(source.updated ?? 0) },
    };
  }
  const error = body("error");
  if (error) return { code: String(error.code ?? ""), message: String(error.message ?? "") };
  return null;
}

export function isLiveFrame(data: WSMessageEvent["data"]): boolean {
  if (typeof data !== "string") return false;
  const message = parseLive(data);
  return message !== null && !("code" in message);
}

/**
 * One desktop's live socket, shared by everything on the page that listens,
 * and reconnected for as long as anything does.
 *
 * Straight to the desktop app on this computer, or through the cloud, which
 * asks the desktop before passing on a single frame. Either way the first
 * frame carries the token — never the URL, which ends up in logs.
 */
export class LiveFeed {
  /** How this socket reaches the desktop, so a changed route can be told apart. */
  readonly route: Connection;
  readonly #hello: string;
  readonly #listeners = new Set<LiveListener>();
  readonly #onRefused: () => void;
  readonly #socket: QWebSocket | null = null;
  // "open" is told once the desktop has said yes — its first frame is where
  // things stand — not when the socket merely connected.
  #welcomed = false;
  #refused = false;
  #closed = false;
  #retry: ReturnType<typeof setTimeout> | null = null;

  constructor(route: Connection, cloud: string, local: string, onRefused: () => void, adapter?: WSAdapter) {
    this.route = route;
    this.#onRefused = onRefused;
    let url: string;
    if (route.via === "local") {
      url = `${local.replace(/^http/, "ws")}/api/live`;
      this.#hello = JSON.stringify({ hello: { role: "portal", token: route.token } });
    } else {
      url = `${cloudSocketBase(cloud)}/api/live`;
      this.#hello = JSON.stringify({ fingerprint: route.fingerprint, token: route.token });
    }
    if (!adapter && typeof WebSocket === "undefined") return;
    const socket = new QWebSocket(url).setReconnect({
      retries: Infinity,
      delay: 1000,
      backoff: "exponential",
      maxDelay: MAX_BACKOFF_MS,
      resetOn: (event) => isLiveFrame(event.data),
      reconnectOn: () => true,
    });
    if (adapter) socket.setAdapter(adapter);
    socket.onOpen(() => {
      this.#welcomed = false;
      this.#refused = false;
      socket.send(this.#hello);
    });
    socket.onMessage((event) => this.#hear(event.data));
    this.#socket = socket;
    this.#start(1000);
  }

  #start(wait: number): void {
    if (this.#closed || !this.#socket) return;
    try {
      this.#socket.connect();
    } catch {
      this.#socket.close();
      this.#retry = setTimeout(() => this.#start(Math.min(MAX_BACKOFF_MS, wait * 2)), wait);
    }
  }

  listen(listener: LiveListener): void {
    this.#listeners.add(listener);
  }

  unlisten(listener: LiveListener): void {
    this.#listeners.delete(listener);
  }

  listeners(): LiveListener[] {
    return [...this.#listeners];
  }

  get listening(): boolean {
    return this.#listeners.size > 0;
  }

  close(): void {
    this.#closed = true;
    if (this.#retry) clearTimeout(this.#retry);
    this.#socket?.close();
  }

  #tell(message: LiveMessage): void {
    for (const listener of [...this.#listeners]) {
      try {
        listener(message);
      } catch {
        // One listener's mistake is not every listener's problem.
      }
    }
  }

  #hear(data: WSMessageEvent["data"]): void {
    const message = parseLive(String(data));
    if (!message) return;
    if ("code" in message) {
      // A token the desktop no longer honours will not start working by
      // asking again; anything else — offline, a restart — may.
      if (message.code === "unauthorised" && !this.#refused) {
        this.#refused = true;
        this.#socket?.close();
        this.#onRefused();
      }
      return;
    }
    if (!this.#welcomed) {
      this.#welcomed = true;
      this.#tell({ kind: "open" });
    }
    this.#tell(message);
  }
}

/** The cloud's socket origin: this page's own, unless one was given. */
function cloudSocketBase(cloud: string): string {
  if (cloud) return cloud.replace(/^http/, "ws");
  if (typeof location === "undefined") return "";
  return `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}`;
}
