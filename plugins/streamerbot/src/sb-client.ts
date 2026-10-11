/**
 * A client for Streamer.bot's WebSocket server (Servers/Clients → WebSocket
 * Server; `ws://127.0.0.1:8080/` by default).
 *
 * The protocol is JSON requests with an `id`, answered with the same `id` and a
 * `status`, plus events pushed for whatever the client subscribed to:
 *
 * * On connect the server sends `Hello`. When authentication is on it carries a
 *   `salt` and a `challenge`, and nothing but `Authenticate` is answered until
 *   the client proves it knows the password.
 * * Events arrive as `{ event: { source, type }, data }`.
 *
 * The socket is injected rather than created here, so the tests run the whole
 * client against an in-memory Streamer.bot with no network.
 */

import { createHash } from "node:crypto";

/** The part of the WebSocket API this client uses. Node's global fits it. */
export interface SocketLike {
  send(data: string): void;
  close(code?: number, reason?: string): void;
  onopen: ((event: unknown) => void) | null;
  onmessage: ((event: { data: unknown }) => void) | null;
  onclose: ((event: { code?: number; reason?: string }) => void) | null;
  onerror: ((event: unknown) => void) | null;
}

export type Connect = (url: string) => SocketLike;

export const defaultConnect: Connect = (url) => new WebSocket(url) as unknown as SocketLike;

export class StreamerbotError extends Error {}

/** `base64(sha256(base64(sha256(password + salt)) + challenge))`. */
export function authResponse(password: string, salt: string, challenge: string): string {
  const secret = createHash("sha256").update(password + salt).digest("base64");
  return createHash("sha256").update(secret + challenge).digest("base64");
}

/** One Streamer.bot action, as `GetActions` lists it. */
export interface SbAction {
  id: string;
  name: string;
  group: string;
  enabled: boolean;
}

export interface SbEvent {
  source: string;
  type: string;
  data: Record<string, unknown>;
}

export interface StreamerbotOptions {
  url: string;
  password?: string;
  connect?: Connect;
  timeoutMs?: number;
}

type Pending = {
  resolve: (data: Record<string, unknown>) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
};

export class StreamerbotClient {
  private socket: SocketLike | null = null;
  private ready = false;
  private nextId = 1;
  private readonly pending = new Map<string, Pending>();
  private readonly listeners = new Set<(event: SbEvent) => void>();
  private readonly closeListeners = new Set<(reason: string) => void>();
  /** What `Hello` said about the server: its version, its name. */
  info: Record<string, unknown> = {};

  private readonly options: StreamerbotOptions;

  constructor(options: StreamerbotOptions) {
    this.options = options;
  }

  get connected(): boolean {
    return this.ready;
  }

  /** Connect, and authenticate when asked. Rejects with a reason a person can act on. */
  open(): Promise<void> {
    const connect = this.options.connect ?? defaultConnect;
    return new Promise((resolve, reject) => {
      let settled = false;
      const fail = (reason: string) => {
        if (!settled) {
          settled = true;
          reject(new StreamerbotError(reason));
        }
      };
      const succeed = () => {
        if (!settled) {
          settled = true;
          this.ready = true;
          resolve();
        }
      };
      let socket: SocketLike;
      try {
        socket = connect(this.options.url);
      } catch (error) {
        fail(`could not reach Streamer.bot at ${this.options.url}: ${String(error)}`);
        return;
      }
      this.socket = socket;

      socket.onerror = () =>
        fail(`could not reach Streamer.bot at ${this.options.url} — is its WebSocket server started?`);
      socket.onclose = (event) => {
        const was = this.ready;
        this.ready = false;
        this.socket = null;
        for (const [, request] of this.pending) {
          clearTimeout(request.timer);
          request.reject(new StreamerbotError("Streamer.bot closed the connection"));
        }
        this.pending.clear();
        const reason = event.reason ? `Streamer.bot closed the connection: ${event.reason}` : "Streamer.bot closed the connection";
        fail(reason);
        if (was) for (const listener of this.closeListeners) listener(reason);
      };
      socket.onmessage = (event) => {
        let message: Record<string, unknown>;
        try {
          message = JSON.parse(String(event.data)) as Record<string, unknown>;
        } catch {
          return;
        }
        if (message.request === "Hello") {
          this.info = (message.info as Record<string, unknown>) ?? {};
          const auth = message.authentication as { salt: string; challenge: string } | undefined;
          if (!auth) {
            succeed();
            return;
          }
          if (!this.options.password) {
            fail("Streamer.bot needs a password; set it in the plugin's settings");
            socket.close();
            return;
          }
          this.send("Authenticate", { authentication: authResponse(this.options.password, auth.salt, auth.challenge) }, true)
            .then(() => succeed())
            .catch(() => {
              fail("Streamer.bot refused the password");
              socket.close();
            });
          return;
        }
        const event_ = message.event as { source?: string; type?: string } | undefined;
        if (event_ && message.id === undefined) {
          const sbEvent: SbEvent = {
            source: String(event_.source ?? ""),
            type: String(event_.type ?? ""),
            data: (message.data as Record<string, unknown>) ?? {},
          };
          for (const listener of this.listeners) listener(sbEvent);
          return;
        }
        const id = message.id === undefined ? undefined : String(message.id);
        const request = id ? this.pending.get(id) : undefined;
        if (!id || !request) return;
        this.pending.delete(id);
        clearTimeout(request.timer);
        if (message.status === "ok") {
          request.resolve(message);
        } else {
          request.reject(new StreamerbotError(String(message.error ?? message.message ?? "Streamer.bot refused the request")));
        }
      };
    });
  }

  /** Send one request and wait for its answer. */
  request(type: string, fields: Record<string, unknown> = {}): Promise<Record<string, unknown>> {
    if (!this.ready) return Promise.reject(new StreamerbotError("Streamer.bot is not connected"));
    return this.send(type, fields, false);
  }

  private send(type: string, fields: Record<string, unknown>, beforeReady: boolean): Promise<Record<string, unknown>> {
    const socket = this.socket;
    if (!socket || (!beforeReady && !this.ready)) {
      return Promise.reject(new StreamerbotError("Streamer.bot is not connected"));
    }
    const id = `fd-${this.nextId++}`;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new StreamerbotError(`Streamer.bot did not answer ${type}`));
      }, this.options.timeoutMs ?? 5000);
      this.pending.set(id, { resolve, reject, timer });
      socket.send(JSON.stringify({ request: type, id, ...fields }));
    });
  }

  async actions(): Promise<SbAction[]> {
    const answer = await this.request("GetActions");
    return ((answer.actions as Record<string, unknown>[]) ?? []).map((action) => ({
      id: String(action.id ?? ""),
      name: String(action.name ?? ""),
      group: String(action.group ?? ""),
      enabled: action.enabled !== false,
    }));
  }

  /** Run an action, by id (preferred) or by name, with arguments. */
  async doAction(action: { id?: string; name?: string }, args: Record<string, string | number | boolean> = {}): Promise<void> {
    await this.request("DoAction", { action, args });
  }

  /**
   * Subscribe to events. Each source is asked for separately: an older
   * Streamer.bot that does not know one refuses only that one.
   */
  async subscribe(events: Record<string, string[]>): Promise<string[]> {
    const refused: string[] = [];
    for (const [source, types] of Object.entries(events)) {
      try {
        await this.request("Subscribe", { events: { [source]: types } });
      } catch {
        refused.push(source);
      }
    }
    return refused;
  }

  /** Persisted global variables, by name. */
  async globals(): Promise<Record<string, unknown>> {
    const answer = await this.request("GetGlobals", { persisted: true });
    const raw = (answer.variables as Record<string, { value?: unknown }>) ?? {};
    return Object.fromEntries(Object.entries(raw).map(([name, entry]) => [name, entry?.value ?? entry]));
  }

  on(listener: (event: SbEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  onClose(listener: (reason: string) => void): () => void {
    this.closeListeners.add(listener);
    return () => this.closeListeners.delete(listener);
  }

  close(): void {
    const socket = this.socket;
    this.socket = null;
    this.ready = false;
    socket?.close(1000, "flexdeck is stopping");
  }
}
