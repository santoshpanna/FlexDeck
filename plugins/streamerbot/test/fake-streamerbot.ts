/**
 * An in-memory Streamer.bot: the server side of its WebSocket API, enough to
 * drive the plugin. Running the helper action really switches the target on or
 * off and announces it, the way Streamer.bot would.
 */

import { randomBytes } from "node:crypto";

import { authResponse, type Connect, type SbAction, type SocketLike } from "../src/sb-client.ts";

export interface FakeOptions {
  password?: string;
  offline?: boolean;
  /** Event sources this Streamer.bot does not offer, to prove one refusal is not fatal. */
  missingSources?: string[];
}

export class FakeStreamerbot {
  actions: SbAction[] = [
    { id: "11111111-1111-1111-1111-111111111111", name: "Shout-out", group: "Chat", enabled: true },
    { id: "22222222-2222-2222-2222-222222222222", name: "Lights on", group: "Scene", enabled: true },
    { id: "33333333-3333-3333-3333-333333333333", name: "Lights off", group: "Scene", enabled: false },
    { id: "44444444-4444-4444-4444-444444444444", name: "flexdeck: set action state", group: "flexdeck", enabled: true },
  ];
  globals: Record<string, string> = { viewers: "12", mood: "calm" };
  /** Every DoAction, in order: the action's name and its arguments. */
  ran: { name: string; args: Record<string, unknown> }[] = [];
  sockets: FakeSocket[] = [];
  offline: boolean;
  readonly options: FakeOptions;

  constructor(options: FakeOptions = {}) {
    this.options = options;
    this.offline = options.offline ?? false;
  }

  readonly connect: Connect = () => {
    const socket = new FakeSocket(this);
    this.sockets.push(socket);
    return socket;
  };

  /** Push an event to every client subscribed to it. */
  event(source: string, type: string, data: Record<string, unknown>): void {
    for (const socket of this.sockets) {
      if (socket.subscribed.get(source)?.includes(type)) {
        socket.deliver({ timeStamp: new Date().toISOString(), event: { source, type }, data });
      }
    }
  }

  quit(): void {
    this.offline = true;
    for (const socket of this.sockets) socket.drop("Streamer.bot is closing");
    this.sockets = [];
  }

  handle(socket: FakeSocket, request: Record<string, unknown>): Record<string, unknown> {
    switch (request.request) {
      case "GetActions":
        return { actions: this.actions.map((action) => ({ ...action, subaction_count: 1 })), count: this.actions.length };
      case "DoAction": {
        const wanted = request.action as { id?: string; name?: string };
        const action = this.actions.find((candidate) => candidate.id === wanted.id || candidate.name === wanted.name);
        if (!action) throw new Error("Action not found");
        const args = (request.args as Record<string, unknown>) ?? {};
        this.ran.push({ name: action.name, args });
        if (action.name === "flexdeck: set action state") {
          const target = this.actions.find((candidate) => candidate.id === args.actionId);
          if (target) {
            target.enabled = Boolean(args.enabled);
            this.event("Application", "ActionUpdated", { id: target.id, name: target.name, enabled: target.enabled });
          }
        }
        this.event("Raw", "ActionCompleted", { actionId: action.id, actionName: action.name });
        return {};
      }
      case "Subscribe": {
        const events = request.events as Record<string, string[]>;
        for (const [source, types] of Object.entries(events)) {
          if (this.options.missingSources?.includes(source)) throw new Error(`Unknown event source ${source}`);
          socket.subscribed.set(source, [...(socket.subscribed.get(source) ?? []), ...types]);
        }
        return {};
      }
      case "GetGlobals":
        return {
          variables: Object.fromEntries(
            Object.entries(this.globals).map(([name, value]) => [name, { name, value, lastWrite: "now" }]),
          ),
          count: Object.keys(this.globals).length,
        };
      default:
        throw new Error(`Unknown request ${String(request.request)}`);
    }
  }
}

export class FakeSocket implements SocketLike {
  onopen: ((event: unknown) => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onclose: ((event: { code?: number; reason?: string }) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  subscribed = new Map<string, string[]>();
  private authenticated: boolean;
  private salt = randomBytes(16).toString("base64");
  private challenge = randomBytes(16).toString("base64");
  private open = true;
  private readonly bot: FakeStreamerbot;

  constructor(bot: FakeStreamerbot) {
    this.bot = bot;
    this.authenticated = !bot.options.password;
    queueMicrotask(() => {
      if (bot.offline) {
        this.onerror?.({});
        this.drop("refused");
        return;
      }
      this.onopen?.({});
      const hello: Record<string, unknown> = { request: "Hello", info: { version: "0.2.5", name: "Streamer.bot" } };
      if (bot.options.password) hello.authentication = { salt: this.salt, challenge: this.challenge };
      this.deliver(hello);
    });
  }

  deliver(message: Record<string, unknown>): void {
    if (this.open) queueMicrotask(() => this.onmessage?.({ data: JSON.stringify(message) }));
  }

  drop(reason: string): void {
    if (!this.open) return;
    this.open = false;
    queueMicrotask(() => this.onclose?.({ code: 1001, reason }));
  }

  send(data: string): void {
    const request = JSON.parse(data) as Record<string, unknown>;
    const reply = (fields: Record<string, unknown>) => this.deliver({ id: request.id, ...fields });
    if (request.request === "Authenticate") {
      const expected = authResponse(this.bot.options.password ?? "", this.salt, this.challenge);
      if (request.authentication === expected) {
        this.authenticated = true;
        reply({ status: "ok" });
      } else {
        reply({ status: "error", error: "Authentication failed" });
      }
      return;
    }
    if (!this.authenticated) {
      reply({ status: "error", error: "Not authenticated" });
      return;
    }
    try {
      reply({ status: "ok", ...this.bot.handle(this, request) });
    } catch (error) {
      reply({ status: "error", error: error instanceof Error ? error.message : String(error) });
    }
  }

  close(): void {
    this.drop("closed by the client");
  }
}
