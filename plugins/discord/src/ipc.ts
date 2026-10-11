/**
 * Discord's local RPC, over the IPC pipe the desktop client opens.
 *
 * Discord listens on `\\?\pipe\discord-ipc-0` (up to `-9` when several
 * clients run) on Windows, and on a Unix socket of the same name elsewhere.
 * Each frame is an 8-byte header — opcode and length, little-endian u32s —
 * followed by that many bytes of JSON:
 *
 * * op 0 HANDSHAKE `{v: 1, client_id}`, answered with a READY dispatch;
 * * op 1 FRAME, both ways: a command `{cmd, args, nonce}`, its answer with the
 *   same nonce (`evt: "ERROR"` when refused), or an event (`cmd: "DISPATCH"`);
 * * op 2 CLOSE, with a reason;
 * * op 3 PING / op 4 PONG.
 *
 * The pipe path can be overridden (`FLEXDECK_DISCORD_PIPE`) so the tests run
 * this against a real named-pipe server that pretends to be Discord.
 */

import { randomUUID } from "node:crypto";
import { connect as netConnect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

const Op = { Handshake: 0, Frame: 1, Close: 2, Ping: 3, Pong: 4 } as const;

export class DiscordError extends Error {
  /** Discord's numeric error code, when it gave one. */
  readonly code: number | undefined;
  /** Whether a Discord answered at all — then its refusal is the answer. */
  reached = false;

  constructor(message: string, code?: number) {
    super(message);
    this.code = code;
  }
}

/** Where Discord's pipe number `n` lives on this platform. */
export function pipePath(n: number): string {
  if (process.platform === "win32") return `\\\\?\\pipe\\discord-ipc-${n}`;
  const base = process.env.XDG_RUNTIME_DIR || process.env.TMPDIR || process.env.TMP || tmpdir();
  return join(base, `discord-ipc-${n}`);
}

/** Encode one frame. */
export function encode(op: number, payload: unknown): Buffer {
  const body = Buffer.from(JSON.stringify(payload), "utf8");
  const header = Buffer.alloc(8);
  header.writeUInt32LE(op, 0);
  header.writeUInt32LE(body.length, 4);
  return Buffer.concat([header, body]);
}

/** Pull every complete frame off the front of `buffer`; the rest is kept. */
export function decode(buffer: Buffer): { frames: { op: number; payload: Record<string, unknown> }[]; rest: Buffer } {
  const frames: { op: number; payload: Record<string, unknown> }[] = [];
  let at = 0;
  while (buffer.length - at >= 8) {
    const op = buffer.readUInt32LE(at);
    const length = buffer.readUInt32LE(at + 4);
    if (buffer.length - at - 8 < length) break;
    const body = buffer.subarray(at + 8, at + 8 + length).toString("utf8");
    at += 8 + length;
    try {
      frames.push({ op, payload: JSON.parse(body) as Record<string, unknown> });
    } catch {
      // A frame that is not JSON is skipped rather than killing the link.
    }
  }
  return { frames, rest: buffer.subarray(at) };
}

type Pending = {
  resolve: (data: Record<string, unknown>) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
};

export interface IpcOptions {
  clientId: string;
  /** One path to try instead of discord-ipc-0..9. */
  path?: string;
  /** For an ordinary command. AUTHORIZE waits for a person and gets longer. */
  timeoutMs?: number;
}

export class DiscordIpc {
  private socket: Socket | null = null;
  private buffer: Buffer = Buffer.alloc(0);
  private readonly pending = new Map<string, Pending>();
  private readonly listeners = new Set<(event: string, data: Record<string, unknown>) => void>();
  private readonly closeListeners = new Set<(reason: string) => void>();
  private readonly options: IpcOptions;
  /** The signed-in user, from READY. */
  user: { id: string; username: string; global_name?: string } | null = null;

  constructor(options: IpcOptions) {
    this.options = options;
  }

  get connected(): boolean {
    return this.socket !== null && this.user !== null;
  }

  /** Find Discord's pipe, shake hands, and wait for READY. */
  async open(): Promise<void> {
    const paths = this.options.path ? [this.options.path] : Array.from({ length: 10 }, (_, n) => pipePath(n));
    let last = "Discord is not running";
    for (const path of paths) {
      try {
        await this.tryPath(path);
        return;
      } catch (error) {
        // A Discord that answered and refused is the answer; trying the next
        // pipe would only replace its reason with "not running".
        if (error instanceof DiscordError && error.reached) throw error;
        last = error instanceof Error ? error.message : String(error);
      }
    }
    throw new DiscordError(last);
  }

  private tryPath(path: string): Promise<void> {
    return new Promise((resolve, reject) => {
      let settled = false;
      let reached = false;
      const fail = (reason: string) => {
        if (!settled) {
          settled = true;
          const error = new DiscordError(reason);
          error.reached = reached;
          reject(error);
        }
      };
      const socket = netConnect(path);
      socket.once("error", () => fail("Discord is not running (or its desktop app is not signed in)"));
      socket.once("connect", () => {
        reached = true;
        this.socket = socket;
        socket.write(encode(Op.Handshake, { v: 1, client_id: this.options.clientId }));
      });
      socket.on("data", (chunk: Buffer) => {
        this.buffer = Buffer.concat([this.buffer, chunk]);
        const { frames, rest } = decode(this.buffer);
        this.buffer = rest;
        for (const frame of frames) {
          if (frame.op === Op.Ping) {
            socket.write(encode(Op.Pong, frame.payload));
          } else if (frame.op === Op.Close) {
            const message = String(frame.payload.message ?? "Discord closed the connection");
            // 4000 on handshake means the client id is not a real application.
            fail(frame.payload.code === 4000 ? "Discord does not know that Client ID" : message);
            socket.end();
          } else if (frame.payload.cmd === "DISPATCH" && frame.payload.evt === "READY") {
            const data = (frame.payload.data as Record<string, unknown>) ?? {};
            this.user = (data.user as DiscordIpc["user"]) ?? { id: "", username: "" };
            if (!settled) {
              settled = true;
              resolve();
            }
          } else {
            this.handle(frame.payload);
          }
        }
      });
      socket.on("close", () => {
        const was = this.connected;
        this.socket = null;
        this.user = null;
        for (const [, request] of this.pending) {
          clearTimeout(request.timer);
          request.reject(new DiscordError("Discord closed the connection"));
        }
        this.pending.clear();
        fail("Discord closed the connection");
        if (was) for (const listener of this.closeListeners) listener("Discord closed the connection");
      });
    });
  }

  private handle(payload: Record<string, unknown>): void {
    if (payload.cmd === "DISPATCH") {
      const event = String(payload.evt ?? "");
      const data = (payload.data as Record<string, unknown>) ?? {};
      for (const listener of this.listeners) listener(event, data);
      return;
    }
    const nonce = payload.nonce === undefined || payload.nonce === null ? undefined : String(payload.nonce);
    const request = nonce ? this.pending.get(nonce) : undefined;
    if (!nonce || !request) return;
    this.pending.delete(nonce);
    clearTimeout(request.timer);
    const data = (payload.data as Record<string, unknown>) ?? {};
    if (payload.evt === "ERROR") {
      request.reject(new DiscordError(String(data.message ?? "Discord refused"), Number(data.code) || undefined));
    } else {
      request.resolve(data);
    }
  }

  /** Send a command and wait for Discord's answer. */
  command(cmd: string, args: Record<string, unknown> = {}, timeoutMs?: number): Promise<Record<string, unknown>> {
    return this.frame({ cmd, args }, timeoutMs);
  }

  /** Subscribe to an event, optionally scoped (a channel's voice states). */
  subscribe(event: string, args: Record<string, unknown> = {}): Promise<Record<string, unknown>> {
    return this.frame({ cmd: "SUBSCRIBE", evt: event, args });
  }

  unsubscribe(event: string, args: Record<string, unknown> = {}): Promise<Record<string, unknown>> {
    return this.frame({ cmd: "UNSUBSCRIBE", evt: event, args });
  }

  private frame(body: Record<string, unknown>, timeoutMs?: number): Promise<Record<string, unknown>> {
    const socket = this.socket;
    if (!socket) return Promise.reject(new DiscordError("Discord is not connected"));
    const nonce = randomUUID();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(nonce);
        reject(new DiscordError(`Discord did not answer ${String(body.cmd)}`));
      }, timeoutMs ?? this.options.timeoutMs ?? 5000);
      this.pending.set(nonce, { resolve, reject, timer });
      socket.write(encode(Op.Frame, { ...body, nonce }));
    });
  }

  on(listener: (event: string, data: Record<string, unknown>) => void): () => void {
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
    this.user = null;
    if (socket) {
      try {
        socket.write(encode(Op.Close, {}));
      } catch {
        // Already gone.
      }
      socket.end();
    }
  }
}
