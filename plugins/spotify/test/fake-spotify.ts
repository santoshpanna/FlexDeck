/**
 * A fake Spotify: the accounts service (token endpoint) and enough of the Web
 * API to drive the plugin, on one local HTTP server.
 */

import { createServer, type Server } from "node:http";

export class FakeSpotify {
  base = "";
  liked = new Set<string>();
  volume = 40;
  premium = true;
  /** How many requests to answer 429 before behaving. */
  throttle = 0;
  /** Make the next API call's token look expired. */
  expireNext = false;
  playlists = [{ id: "pl1", name: "Road trip" }];
  added: { playlist: string; uris: string[] }[] = [];
  played: Record<string, unknown>[] = [];
  transferred: string[] = [];
  tokenRequests: Record<string, string>[] = [];
  issued = 0;
  /** What the plugin asked for, method and path. */
  calls: string[] = [];
  private server: Server | null = null;
  private valid = new Set<string>();

  async start(): Promise<void> {
    this.server = createServer((request, response) => {
      let body = "";
      request.on("data", (chunk) => (body += chunk));
      request.on("end", () => this.handle(request.method ?? "GET", request.url ?? "/", request.headers, body, response));
    });
    await new Promise<void>((resolve) => this.server!.listen(0, "127.0.0.1", resolve));
    const address = this.server.address();
    this.base = typeof address === "object" && address ? `http://127.0.0.1:${address.port}` : "";
  }

  async stop(): Promise<void> {
    this.server?.closeAllConnections();
    await new Promise<void>((resolve) => (this.server ? this.server.close(() => resolve()) : resolve()));
  }

  private handle(
    method: string,
    rawUrl: string,
    headers: Record<string, string | string[] | undefined>,
    body: string,
    response: import("node:http").ServerResponse,
  ): void {
    const url = new URL(rawUrl, "http://x");
    response.setHeader("Connection", "close");
    const json = (status: number, value: unknown) => {
      response.statusCode = status;
      response.setHeader("Content-Type", "application/json");
      response.end(JSON.stringify(value));
    };
    if (url.pathname === "/api/token") {
      const fields = Object.fromEntries(new URLSearchParams(body));
      this.tokenRequests.push(fields);
      const ok =
        (fields.grant_type === "authorization_code" && fields.code === "the-code" && fields.code_verifier) ||
        (fields.grant_type === "refresh_token" && fields.refresh_token?.startsWith("refresh-"));
      if (!ok) return json(400, { error: "invalid_grant", error_description: "Invalid authorization code" });
      this.issued += 1;
      this.valid.add(`access-${this.issued}`);
      return json(200, { access_token: `access-${this.issued}`, refresh_token: `refresh-${this.issued}`, expires_in: 3600 });
    }
    if (url.pathname === "/art.jpg") {
      response.setHeader("Content-Type", "image/jpeg");
      response.end(Buffer.from([0xff, 0xd8, 0xff, 0xe0, 1, 2, 3]));
      return;
    }
    this.calls.push(`${method} ${url.pathname}`);
    const auth = String(headers.authorization ?? "").replace("Bearer ", "");
    if (this.expireNext) {
      this.expireNext = false;
      this.valid.delete(auth);
    }
    if (!this.valid.has(auth)) return json(401, { error: { status: 401, message: "The access token expired" } });
    if (this.throttle > 0) {
      this.throttle -= 1;
      response.setHeader("Retry-After", "2");
      return json(429, { error: { status: 429, message: "API rate limit exceeded" } });
    }
    const track = { id: "t1", uri: "spotify:track:t1", name: "Song", artists: [{ name: "Ada" }, { name: "Grace" }], album: { images: [{ url: `${this.base}/art.jpg`, width: 300 }, { url: `${this.base}/big.jpg`, width: 640 }] } };
    switch (`${method} ${url.pathname}`) {
      case "GET /v1/me":
        return json(200, { id: "me", display_name: "Me", product: this.premium ? "premium" : "free" });
      case "GET /v1/me/player":
        return json(200, { is_playing: true, item: track, device: { id: "d1", name: "Desktop", volume_percent: this.volume } });
      case "GET /v1/me/player/currently-playing":
        return json(200, { item: track });
      case "GET /v1/me/tracks/contains":
        return json(200, [this.liked.has(String(url.searchParams.get("ids")))]);
      case "PUT /v1/me/tracks":
        this.liked.add(String(url.searchParams.get("ids")));
        return json(200, {});
      case "DELETE /v1/me/tracks":
        this.liked.delete(String(url.searchParams.get("ids")));
        return json(200, {});
      case "GET /v1/me/playlists":
        return json(200, { items: this.playlists });
      case "PUT /v1/me/player/play":
        this.played.push(JSON.parse(body || "{}"));
        response.statusCode = 204;
        return void response.end();
      case "PUT /v1/me/player/volume":
        if (!this.premium) return json(403, { error: { status: 403, message: "Player command failed: Premium required", reason: "PREMIUM_REQUIRED" } });
        this.volume = Number(url.searchParams.get("volume_percent"));
        response.statusCode = 204;
        return void response.end();
      case "GET /v1/me/player/devices":
        return json(200, { devices: [{ id: "d1", name: "Desktop", type: "Computer" }, { id: "d2", name: "Kitchen", type: "Speaker" }] });
      case "PUT /v1/me/player":
        this.transferred.push(...((JSON.parse(body) as { device_ids: string[] }).device_ids));
        response.statusCode = 204;
        return void response.end();
    }
    if (method === "POST" && url.pathname.startsWith("/v1/playlists/")) {
      this.added.push({ playlist: url.pathname.split("/")[3]!, uris: (JSON.parse(body) as { uris: string[] }).uris });
      return json(201, { snapshot_id: "s" });
    }
    return json(404, { error: { status: 404, message: "Not found" } });
  }
}
