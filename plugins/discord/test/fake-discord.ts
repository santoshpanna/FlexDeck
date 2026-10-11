/**
 * A fake Discord desktop client: a real named-pipe (or Unix socket) server
 * speaking Discord's IPC framing, plus a fake OAuth token endpoint. Enough of
 * RPC to drive the plugin, with the state changes announced as DISPATCH
 * events the way Discord does.
 */

import { randomUUID } from "node:crypto";
import { createServer as createHttp, type Server as HttpServer } from "node:http";
import { createServer, type Server, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { decode, encode } from "../src/ipc.ts";

export interface FakeDiscordOptions {
  /** Refuse AUTHORIZE, as if the person pressed Cancel. */
  deny?: boolean;
  /** Commands this "Discord" does not know, to test feature detection. */
  unknown?: string[];
  /** Require a client secret at the token endpoint (a confidential app). */
  requireSecret?: boolean;
}

interface Client {
  socket: Socket;
  buffer: Buffer;
  authenticated: boolean;
  subscriptions: Set<string>;
}

export class FakeDiscord {
  readonly pipe: string;
  apiBase = "";
  voice = {
    mute: false,
    deaf: false,
    mode: { type: "VOICE_ACTIVITY" },
    input: { device_id: "default", volume: 80, available_devices: [{ id: "default", name: "Default" }, { id: "mic-2", name: "Microphone (Yeti)" }] },
    output: { device_id: "default", volume: 100, available_devices: [{ id: "default", name: "Default" }, { id: "spk-2", name: "Headphones" }] },
  };
  channel: string | null = "200";
  members = new Map([
    ["1", { user: { id: "1", username: "me" }, nick: "Me", volume: 100, mute: false }],
    ["2", { user: { id: "2", username: "ada", global_name: "Ada" }, nick: "", volume: 100, mute: false }],
  ]);
  /** Every command a client sent, in order, with its arguments. */
  commands: { cmd: string; args: Record<string, unknown> }[] = [];
  /** Every token-endpoint request's form fields. */
  tokenRequests: Record<string, string>[] = [];
  issued = 0;
  private server: Server | null = null;
  private http: HttpServer | null = null;
  private readonly clients = new Set<Client>();
  readonly options: FakeDiscordOptions;

  constructor(options: FakeDiscordOptions = {}) {
    this.options = options;
    const name = `flexdeck-test-discord-${randomUUID()}`;
    this.pipe = process.platform === "win32" ? `\\\\.\\pipe\\${name}` : join(tmpdir(), `${name}.sock`);
  }

  async start(): Promise<void> {
    this.server = createServer((socket) => this.accept(socket));
    await new Promise<void>((resolve) => this.server!.listen(this.pipe, resolve));
    this.http = createHttp((request, response) => {
      let body = "";
      request.on("data", (chunk) => (body += chunk));
      request.on("end", () => {
        const fields = Object.fromEntries(new URLSearchParams(body));
        this.tokenRequests.push(fields);
        const ok =
          (fields.grant_type === "authorization_code" && fields.code === "the-code") ||
          (fields.grant_type === "refresh_token" && fields.refresh_token?.startsWith("refresh-"));
        const secretOk = !this.options.requireSecret || fields.client_secret === "s3cret";
        response.setHeader("Content-Type", "application/json");
        response.setHeader("Connection", "close");
        if (!ok || !secretOk) {
          response.statusCode = 400;
          response.end(JSON.stringify({ error: "invalid_grant", error_description: "Invalid code" }));
          return;
        }
        this.issued += 1;
        response.end(
          JSON.stringify({ access_token: `access-${this.issued}`, refresh_token: `refresh-${this.issued}`, expires_in: 604800 }),
        );
      });
    });
    await new Promise<void>((resolve) => this.http!.listen(0, "127.0.0.1", resolve));
    const address = this.http.address();
    this.apiBase = typeof address === "object" && address ? `http://127.0.0.1:${address.port}` : "";
  }

  async stop(): Promise<void> {
    for (const client of this.clients) client.socket.destroy();
    this.clients.clear();
    await new Promise<void>((resolve) => (this.server ? this.server.close(() => resolve()) : resolve()));
    // fetch keeps its connection alive; close() alone would wait on it forever.
    this.http?.closeAllConnections();
    await new Promise<void>((resolve) => (this.http ? this.http.close(() => resolve()) : resolve()));
  }

  /** Drop every client, as if Discord quit. */
  quit(): void {
    for (const client of this.clients) client.socket.destroy();
    this.clients.clear();
  }

  /** Send an event to every client subscribed to it. */
  dispatch(evt: string, data: Record<string, unknown>): void {
    for (const client of this.clients) {
      if (client.subscriptions.has(evt)) this.send(client, { cmd: "DISPATCH", evt, data });
    }
  }

  private send(client: Client, payload: Record<string, unknown>, op = 1): void {
    client.socket.write(encode(op, payload));
  }

  private accept(socket: Socket): void {
    const client: Client = { socket, buffer: Buffer.alloc(0), authenticated: false, subscriptions: new Set() };
    this.clients.add(client);
    socket.on("data", (chunk: Buffer) => {
      client.buffer = Buffer.concat([client.buffer, chunk]);
      const { frames, rest } = decode(client.buffer);
      client.buffer = rest;
      for (const frame of frames) this.frame(client, frame.op, frame.payload);
    });
    socket.on("close", () => this.clients.delete(client));
    socket.on("error", () => this.clients.delete(client));
  }

  private frame(client: Client, op: number, payload: Record<string, unknown>): void {
    if (op === 0) {
      if (!payload.client_id) {
        this.send(client, { code: 4000, message: "Invalid Client ID" }, 2);
        client.socket.end();
        return;
      }
      this.send(client, { cmd: "DISPATCH", evt: "READY", data: { v: 1, user: { id: "1", username: "me" } } });
      return;
    }
    if (op !== 1) return;
    const cmd = String(payload.cmd);
    const args = (payload.args as Record<string, unknown>) ?? {};
    const nonce = payload.nonce;
    this.commands.push({ cmd, args });
    const reply = (data: Record<string, unknown>) => this.send(client, { cmd, data, evt: null, nonce });
    const error = (code: number, message: string) => this.send(client, { cmd, data: { code, message }, evt: "ERROR", nonce });

    if (this.options.unknown?.includes(cmd)) return error(4002, `Invalid command: ${cmd}`);
    if (cmd === "AUTHORIZE") {
      if (this.options.deny) return error(5000, "OAuth2 Error: access_denied: User cancelled");
      return reply({ code: "the-code" });
    }
    if (cmd === "AUTHENTICATE") {
      if (!String(args.access_token ?? "").startsWith("access-")) return error(4009, "Invalid OAuth2 access token");
      client.authenticated = true;
      return reply({ user: { id: "1", username: "me" }, scopes: [] });
    }
    if (!client.authenticated) return error(4006, "Not authenticated or invalid scope");

    switch (cmd) {
      case "SUBSCRIBE":
        client.subscriptions.add(String(payload.evt));
        return reply({ evt: payload.evt });
      case "UNSUBSCRIBE":
        client.subscriptions.delete(String(payload.evt));
        return reply({ evt: payload.evt });
      case "GET_VOICE_SETTINGS":
        return reply(this.voice);
      case "SET_VOICE_SETTINGS": {
        if (typeof args.mute === "boolean") this.voice.mute = args.mute;
        if (typeof args.deaf === "boolean") this.voice.deaf = args.deaf;
        if (args.mode) this.voice.mode = args.mode as { type: string };
        const input = args.input as Record<string, unknown> | undefined;
        const output = args.output as Record<string, unknown> | undefined;
        if (input?.volume !== undefined) this.voice.input.volume = Number(input.volume);
        if (input?.device_id !== undefined) this.voice.input.device_id = String(input.device_id);
        if (output?.volume !== undefined) this.voice.output.volume = Number(output.volume);
        if (output?.device_id !== undefined) this.voice.output.device_id = String(output.device_id);
        reply(this.voice);
        this.dispatch("VOICE_SETTINGS_UPDATE", this.voice);
        return;
      }
      case "GET_SELECTED_VOICE_CHANNEL":
        return reply(this.channel ? this.channelData(this.channel) : {});
      case "GET_CHANNEL":
        return reply(this.channelData(String(args.channel_id)));
      case "SELECT_VOICE_CHANNEL":
        this.channel = args.channel_id ? String(args.channel_id) : null;
        reply(this.channel ? this.channelData(this.channel) : {});
        this.dispatch("VOICE_CHANNEL_SELECT", { channel_id: this.channel, guild_id: this.channel ? "100" : null });
        return;
      case "SELECT_TEXT_CHANNEL":
        return reply({ id: args.channel_id });
      case "GET_GUILD":
        return reply({ id: args.guild_id, name: "Test Server" });
      case "GET_GUILDS":
        return reply({ guilds: [{ id: "100", name: "Test Server" }] });
      case "GET_CHANNELS":
        return reply({
          channels: [
            { id: "200", name: "General", type: 2 },
            { id: "201", name: "Gaming", type: 2 },
            { id: "300", name: "chat", type: 0 },
          ],
        });
      case "SET_USER_VOICE_SETTINGS": {
        const member = this.members.get(String(args.user_id));
        if (!member) return error(4000, "Unknown user");
        if (args.volume !== undefined) member.volume = Number(args.volume);
        if (typeof args.mute === "boolean") member.mute = args.mute;
        reply({ user_id: args.user_id, volume: member.volume, mute: member.mute });
        this.dispatch("VOICE_STATE_UPDATE", member);
        return;
      }
      case "GET_SOUNDBOARD_SOUNDS":
        return reply({ sounds: [{ sound_id: "900", name: "Airhorn", guild_id: "100" }] } as Record<string, unknown>);
      case "PLAY_SOUNDBOARD_SOUND":
      case "TOGGLE_VIDEO":
      case "TOGGLE_SCREENSHARE":
        return reply({});
      default:
        return error(4002, `Invalid command: ${cmd}`);
    }
  }

  private channelData(id: string): Record<string, unknown> {
    return {
      id,
      guild_id: "100",
      name: id === "201" ? "Gaming" : "General",
      type: 2,
      voice_states: id === this.channel ? [...this.members.values()] : [],
    };
  }
}
