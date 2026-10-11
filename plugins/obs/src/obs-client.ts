/**
 * A client for obs-websocket 5 — the WebSocket server built into OBS Studio
 * 28 and later (Tools → WebSocket Server Settings).
 *
 * The protocol is small: OBS says Hello (op 0) with an optional auth
 * challenge; the client Identifies (op 1); OBS confirms (op 2). After that the
 * client sends Requests (op 6) and gets RequestResponses (op 7), and OBS
 * pushes Events (op 5).
 *
 * The socket is injected rather than created here, so the tests can run this
 * whole client against an in-memory OBS without a network or OBS installed.
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

export const defaultConnect: Connect = (url) =>
  new WebSocket(url, "obswebsocket.json") as unknown as SocketLike;

const Op = { Hello: 0, Identify: 1, Identified: 2, Event: 5, Request: 6, RequestResponse: 7 } as const;

/** Every event category except the high-volume ones (meters, active state). */
const EVENT_SUBSCRIPTIONS = 0x7ff;

export interface ObsOptions {
  url: string;
  password?: string;
  connect?: Connect;
  /** How long a request may take before it is given up on. */
  timeoutMs?: number;
}

export class ObsError extends Error {}

/** `base64(sha256(base64(sha256(password + salt)) + challenge))`, per the spec. */
export function authResponse(password: string, salt: string, challenge: string): string {
  const secret = createHash("sha256").update(password + salt).digest("base64");
  return createHash("sha256").update(secret + challenge).digest("base64");
}

type Pending = { resolve: (data: Record<string, unknown>) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> };

export class ObsClient {
  private socket: SocketLike | null = null;
  private identified = false;
  private nextId = 1;
  private readonly pending = new Map<string, Pending>();
  private readonly listeners = new Set<(type: string, data: Record<string, unknown>) => void>();

  private readonly options: ObsOptions;

  constructor(options: ObsOptions) {
    this.options = options;
  }

  get connected(): boolean {
    return this.identified;
  }

  /** Connect and identify. Rejects with a reason a person can act on. */
  open(): Promise<void> {
    const connect = this.options.connect ?? defaultConnect;
    return new Promise((resolve, reject) => {
      let settled = false;
      const fail = (reason: string) => {
        if (!settled) {
          settled = true;
          reject(new ObsError(reason));
        }
      };
      let socket: SocketLike;
      try {
        socket = connect(this.options.url);
      } catch (error) {
        fail(`could not reach OBS at ${this.options.url}: ${String(error)}`);
        return;
      }
      this.socket = socket;

      socket.onerror = () => fail(`could not reach OBS at ${this.options.url} — is OBS running with its WebSocket server on?`);
      socket.onclose = (event) => {
        const wasIdentified = this.identified;
        this.identified = false;
        this.socket = null;
        for (const [, request] of this.pending) {
          clearTimeout(request.timer);
          request.reject(new ObsError("OBS closed the connection"));
        }
        this.pending.clear();
        // 4009 is obs-websocket's "authentication failed".
        if (event.code === 4009) fail("OBS refused the password");
        else fail(`OBS closed the connection${event.reason ? `: ${event.reason}` : ""}`);
        if (wasIdentified) this.emit("ConnectionClosed", { code: event.code ?? 0, reason: event.reason ?? "" });
      };
      socket.onmessage = (event) => {
        let message: { op: number; d: Record<string, unknown> };
        try {
          message = JSON.parse(String(event.data));
        } catch {
          return;
        }
        switch (message.op) {
          case Op.Hello: {
            const auth = message.d.authentication as { challenge: string; salt: string } | undefined;
            if (auth && !this.options.password) {
              fail("OBS needs a password; set it in the plugin's settings");
              socket.close();
              return;
            }
            const identify: Record<string, unknown> = { rpcVersion: 1, eventSubscriptions: EVENT_SUBSCRIPTIONS };
            if (auth) identify.authentication = authResponse(this.options.password ?? "", auth.salt, auth.challenge);
            socket.send(JSON.stringify({ op: Op.Identify, d: identify }));
            break;
          }
          case Op.Identified:
            this.identified = true;
            settled = true;
            resolve();
            break;
          case Op.Event:
            this.emit(String(message.d.eventType), (message.d.eventData as Record<string, unknown>) ?? {});
            break;
          case Op.RequestResponse: {
            const id = String(message.d.requestId);
            const request = this.pending.get(id);
            if (!request) return;
            this.pending.delete(id);
            clearTimeout(request.timer);
            const status = message.d.requestStatus as { result: boolean; code: number; comment?: string };
            if (status.result) {
              request.resolve((message.d.responseData as Record<string, unknown>) ?? {});
            } else {
              request.reject(new ObsError(status.comment ?? `OBS refused ${String(message.d.requestType)} (code ${status.code})`));
            }
            break;
          }
        }
      };
    });
  }

  /** Send one request and wait for its answer. */
  request<T = Record<string, unknown>>(type: string, data?: Record<string, unknown>): Promise<T> {
    const socket = this.socket;
    if (!socket || !this.identified) return Promise.reject(new ObsError("OBS is not connected"));
    const id = String(this.nextId++);
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new ObsError(`OBS did not answer ${type}`));
      }, this.options.timeoutMs ?? 5000);
      this.pending.set(id, { resolve: resolve as (data: Record<string, unknown>) => void, reject, timer });
      socket.send(JSON.stringify({ op: Op.Request, d: { requestType: type, requestId: id, requestData: data } }));
    });
  }

  /** Hear every event; returns a function that stops listening. */
  on(listener: (type: string, data: Record<string, unknown>) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  close(): void {
    const socket = this.socket;
    this.socket = null;
    this.identified = false;
    socket?.close(1000, "flexdeck is stopping");
  }

  private emit(type: string, data: Record<string, unknown>): void {
    for (const listener of this.listeners) listener(type, data);
  }
}
