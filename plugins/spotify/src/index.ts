/**
 * Spotify for flexdeck.
 *
 * The things Windows' media controls cannot do: like or unlike the song that is
 * playing, add it to a playlist, start a playlist or an album, set Spotify's own
 * volume, and move playback to another device — and on the deck, the track,
 * the artist, whether you have liked it, and its album art.
 *
 * Play, pause and skip work without this plugin, for Spotify and every other
 * player, through the built-in Media plugin.
 *
 * # Signing in, without a secret
 *
 * Spotify's Web API needs a sign-in. This uses PKCE: the plugin opens Spotify's
 * consent page in your browser, Spotify sends the browser back to a small
 * server this plugin runs on `127.0.0.1` for a moment, and the code it carries
 * is exchanged for tokens with a one-time verifier. There is no client secret
 * anywhere, and the refresh token stays in this plugin's settings on your
 * computer.
 *
 * Spotify only lets an application in development mode serve the people it
 * lists, so you use your own (free, two minutes — see the README).
 */

import { createHash, randomBytes } from "node:crypto";
import { createServer, type Server } from "node:http";

import { definePlugin, type Context } from "@flexdeck/plugin-sdk";

const ID = "spotify";

const SCOPES = [
  "user-read-playback-state",
  "user-modify-playback-state",
  "user-read-currently-playing",
  "user-library-read",
  "user-library-modify",
  "playlist-read-private",
  "playlist-modify-private",
  "playlist-modify-public",
  "user-read-private",
];

/** The player is read at most this often, however many widgets watch it. */
const POLL_MS = 3000;
/** How long the consent page may take. */
const SIGN_IN_MS = 180_000;

interface State {
  accessToken: string;
  expiresAt: number;
  /** Spotify asked us to slow down until then. */
  backoffUntil: number;
  polledAt: number;
  player: Record<string, unknown> | null;
  artUrl: string;
  signingIn: Promise<void> | null;
}

const fresh = (): State => ({
  accessToken: "",
  expiresAt: 0,
  backoffUntil: 0,
  polledAt: 0,
  player: null,
  artUrl: "",
  signingIn: null,
});

const state: State = fresh();

/** For the tests: forget everything between cases. */
export function reset(): void {
  Object.assign(state, fresh());
}

const accounts = (ctx: Context) => (ctx.settings.accounts_base || "https://accounts.spotify.com").replace(/\/+$/, "");
const api = (ctx: Context) => (ctx.settings.api_base || "https://api.spotify.com").replace(/\/+$/, "");

export function pkce(): { verifier: string; challenge: string } {
  const verifier = randomBytes(48).toString("base64url");
  return { verifier, challenge: createHash("sha256").update(verifier).digest("base64url") };
}

async function token(ctx: Context, fields: Record<string, string>): Promise<void> {
  const response = await fetch(`${accounts(ctx)}/api/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ client_id: ctx.settings.client_id ?? "", ...fields }),
  });
  const answer = (await response.json().catch(() => ({}))) as Record<string, unknown>;
  if (!response.ok) {
    throw new Error(`Spotify refused the sign-in: ${String(answer.error_description ?? answer.error ?? response.status)}`);
  }
  state.accessToken = String(answer.access_token ?? "");
  state.expiresAt = Date.now() + Number(answer.expires_in ?? 3600) * 1000;
  // Spotify rotates the refresh token: the new one must be kept, or the next
  // start finds the old one revoked.
  const next = String(answer.refresh_token ?? "");
  if (next && next !== ctx.settings.refresh_token) ctx.storeSettings({ refresh_token: next });
}

async function accessToken(ctx: Context): Promise<string> {
  if (state.accessToken && state.expiresAt > Date.now() + 30_000) return state.accessToken;
  if (!ctx.settings.refresh_token) throw new Error("not signed in to Spotify yet — press a Spotify button set to 'Connect'");
  await token(ctx, { grant_type: "refresh_token", refresh_token: ctx.settings.refresh_token });
  return state.accessToken;
}

/** Call the Web API: refresh once on 401, respect 429, and speak plainly on 403. */
async function call(ctx: Context, method: string, path: string, body?: unknown): Promise<Record<string, unknown> | null> {
  if (Date.now() < state.backoffUntil) {
    throw new Error(`Spotify asked to slow down; try again in ${Math.ceil((state.backoffUntil - Date.now()) / 1000)}s`);
  }
  for (let attempt = 0; attempt < 2; attempt++) {
    const response = await fetch(`${api(ctx)}${path}`, {
      method,
      headers: {
        Authorization: `Bearer ${await accessToken(ctx)}`,
        ...(body === undefined ? {} : { "Content-Type": "application/json" }),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (response.status === 401 && attempt === 0) {
      state.accessToken = "";
      continue;
    }
    if (response.status === 429) {
      const wait = Number(response.headers.get("retry-after") ?? "5");
      state.backoffUntil = Date.now() + Math.max(1, Number.isFinite(wait) ? wait : 5) * 1000;
      throw new Error(`Spotify asked to slow down for ${Math.ceil((state.backoffUntil - Date.now()) / 1000)}s`);
    }
    if (response.status === 204 || response.status === 202) return null;
    const text = await response.text();
    const answer = (text ? JSON.parse(text) : {}) as Record<string, unknown>;
    if (!response.ok) {
      const error = answer.error as { message?: string; reason?: string } | undefined;
      if (response.status === 403 && /premium/i.test(`${error?.reason ?? ""} ${error?.message ?? ""}`)) {
        throw new Error("that needs Spotify Premium");
      }
      if (response.status === 404 && /device/i.test(error?.message ?? "")) {
        throw new Error("Spotify is not playing anywhere right now — start it on a device first");
      }
      throw new Error(`Spotify refused: ${error?.message ?? response.status}`);
    }
    return answer;
  }
  throw new Error("Spotify would not accept the sign-in");
}

// --- signing in ----------------------------------------------------------

/**
 * Open Spotify's consent page and wait for the browser to come back to our
 * loopback server with a code. Port `0` (tests only) picks a free port.
 */
export async function signIn(ctx: Context): Promise<void> {
  const clientId = ctx.settings.client_id?.trim();
  if (!clientId) throw new Error("set your Spotify app's Client ID in this plugin's settings (see the README)");
  const { verifier, challenge } = pkce();
  const expected = randomBytes(16).toString("hex");
  const wantedPort = Number(ctx.settings.redirect_port || "47823");

  let server: Server | null = null;
  const code = await new Promise<{ code: string; redirect: string }>((resolve, reject) => {
    let redirect = "";
    const timer = setTimeout(() => reject(new Error("the Spotify sign-in was not finished in time")), SIGN_IN_MS);
    server = createServer((request, response) => {
      const url = new URL(request.url ?? "/", "http://127.0.0.1");
      if (url.pathname !== "/callback") {
        response.statusCode = 404;
        response.end();
        return;
      }
      const ok = url.searchParams.get("state") === expected && url.searchParams.get("code");
      response.setHeader("Content-Type", "text/html; charset=utf-8");
      response.end(
        ok
          ? "<h1>flexdeck is connected to Spotify</h1><p>You can close this tab.</p>"
          : "<h1>Spotify sign-in did not finish</h1><p>Press Connect on the deck to try again.</p>",
      );
      clearTimeout(timer);
      if (ok) resolve({ code: String(url.searchParams.get("code")), redirect });
      else reject(new Error(`Spotify sign-in was refused: ${url.searchParams.get("error") ?? "the answer did not match"}`));
    });
    server.on("error", (error) => {
      clearTimeout(timer);
      reject(new Error(`could not listen on 127.0.0.1:${wantedPort} for Spotify's answer: ${error.message}`));
    });
    server.listen(wantedPort, "127.0.0.1", () => {
      const address = server!.address();
      const port = typeof address === "object" && address ? address.port : wantedPort;
      // Spotify requires the loopback address spelled out, not "localhost".
      redirect = `http://127.0.0.1:${port}/callback`;
      const authorize = new URL(`${accounts(ctx)}/authorize`);
      authorize.search = new URLSearchParams({
        client_id: clientId,
        response_type: "code",
        redirect_uri: redirect,
        code_challenge_method: "S256",
        code_challenge: challenge,
        scope: SCOPES.join(" "),
        state: expected,
      }).toString();
      ctx.openUrl(authorize.toString());
      ctx.log.info("opened Spotify's sign-in page in your browser");
    });
  }).finally(() => (server as Server | null)?.close());

  await token(ctx, {
    grant_type: "authorization_code",
    code: code.code,
    redirect_uri: code.redirect,
    code_verifier: verifier,
  });
  await welcome(ctx);
}

async function welcome(ctx: Context): Promise<void> {
  const me = await call(ctx, "GET", "/v1/me");
  ctx.vars.set({ "spotify.connected": true, "spotify.premium": me?.product === "premium" });
  ctx.log.info(`signed in to Spotify as ${String(me?.display_name ?? me?.id ?? "you")}`);
}

/** Sign in once at a time; a second press while the page is open waits for it. */
function connect(ctx: Context): Promise<void> {
  state.signingIn ??= signIn(ctx).finally(() => (state.signingIn = null));
  return state.signingIn;
}

// --- what is playing -----------------------------------------------------

async function current(ctx: Context): Promise<{ id: string; uri: string; name: string }> {
  const playing = await call(ctx, "GET", "/v1/me/player/currently-playing");
  const item = playing?.item as { id?: string; uri?: string; name?: string } | undefined;
  if (!item?.id) throw new Error("nothing is playing on Spotify");
  return { id: item.id, uri: item.uri ?? `spotify:track:${item.id}`, name: item.name ?? "" };
}

async function isLiked(ctx: Context, id: string): Promise<boolean> {
  const answer = (await call(ctx, "GET", `/v1/me/tracks/contains?ids=${encodeURIComponent(id)}`)) as unknown;
  return Array.isArray(answer) ? Boolean(answer[0]) : false;
}

/** Read the player (at most every few seconds) and publish it. */
async function poll(ctx: Context, wantLiked: boolean): Promise<void> {
  if (Date.now() - state.polledAt < POLL_MS) return;
  state.polledAt = Date.now();
  const player = await call(ctx, "GET", "/v1/me/player");
  state.player = player;
  const item = (player?.item as Record<string, unknown> | undefined) ?? null;
  const device = (player?.device as Record<string, unknown> | undefined) ?? null;
  const artists = ((item?.artists as { name: string }[]) ?? []).map((artist) => artist.name).join(", ");
  ctx.vars.set({
    "spotify.connected": true,
    "spotify.playing": Boolean(player?.is_playing),
    "spotify.track": String(item?.name ?? ""),
    "spotify.artist": artists,
    "spotify.device": String(device?.name ?? ""),
    "spotify.volume": Number(device?.volume_percent ?? 0),
  });
  if (wantLiked && item?.id) ctx.vars.set({ "spotify.liked": await isLiked(ctx, String(item.id)) });

  // The smallest cover that is still sharp on a button; fetched once per album.
  const images = ((item?.album as { images?: { url: string; width?: number }[] } | undefined)?.images ?? [])
    .slice()
    .sort((a, b) => (a.width ?? 0) - (b.width ?? 0));
  const cover = images.find((image) => (image.width ?? 0) >= 200) ?? images.at(-1);
  if (!cover) {
    ctx.vars.set({ "spotify.art": "" });
  } else if (cover.url !== state.artUrl) {
    state.artUrl = cover.url;
    try {
      const response = await fetch(cover.url);
      const bytes = new Uint8Array(await response.arrayBuffer());
      const mime = response.headers.get("content-type")?.startsWith("image/png") ? "image/png" : "image/jpeg";
      ctx.vars.set({ "spotify.art": ctx.images.put(bytes, mime) });
    } catch (error) {
      ctx.log.warn(`could not fetch the album art: ${String(error)}`);
    }
  }
}

// --- suggestions ---------------------------------------------------------

async function playlistChoices(ctx: Context): Promise<{ value: string; label: string }[]> {
  const answer = await call(ctx, "GET", "/v1/me/playlists?limit=50");
  return ((answer?.items as { id: string; name: string }[]) ?? []).map((playlist) => ({ value: playlist.id, label: playlist.name }));
}

async function deviceChoices(ctx: Context): Promise<{ value: string; label: string }[]> {
  const answer = await call(ctx, "GET", "/v1/me/player/devices");
  return ((answer?.devices as { id: string; name: string; type: string }[]) ?? []).map((device) => ({
    value: device.id,
    label: `${device.name} (${device.type.toLowerCase()})`,
  }));
}

/** A playlist, album or track as Spotify wants it in a play request. */
export function playBody(uri: string): Record<string, unknown> {
  const trimmed = uri.trim();
  const fromLink = /open\.spotify\.com\/(track|album|playlist|artist|episode|show)\/([A-Za-z0-9]+)/.exec(trimmed);
  const normal = fromLink ? `spotify:${fromLink[1]}:${fromLink[2]}` : trimmed;
  if (!/^spotify:(track|album|playlist|artist|episode|show):[A-Za-z0-9]+$/.test(normal)) {
    throw new Error(`'${uri}' is not a Spotify link or URI`);
  }
  return /^spotify:(track|episode):/.test(normal) ? { uris: [normal] } : { context_uri: normal };
}

let contextHolder: Context | null = null;
const ctxOf = () => {
  if (!contextHolder) throw new Error("the plugin has not started");
  return contextHolder;
};

const plugin = definePlugin({
  id: ID,
  name: "Spotify",
  version: "0.1.0",
  description: "Like songs, add them to playlists, start playlists, Spotify's volume and devices — and the album art on the deck.",
  author: "flexdeck",
  permissions: ["network"],

  settings: {
    client_id: { kind: "text", description: "Your Spotify app's Client ID (see the README)", default: "" },
    redirect_port: {
      kind: "number",
      description: "The port in your app's Redirect URI (http://127.0.0.1:<port>/callback)",
      default: "47823",
    },
    refresh_token: { kind: "text", description: "Kept by the plugin; clear it to sign out", secret: true },
    accounts_base: { kind: "text", description: "Spotify's sign-in service (leave as it is)", default: "https://accounts.spotify.com" },
    api_base: { kind: "text", description: "Spotify's Web API (leave as it is)", default: "https://api.spotify.com" },
  },

  variables: {
    "spotify.connected": { kind: "bool", description: "Whether flexdeck is signed in to Spotify" },
    "spotify.premium": { kind: "bool", description: "Whether the account has Premium (needed to control playback)" },
    "spotify.playing": { kind: "bool", description: "Whether Spotify is playing" },
    "spotify.track": { kind: "text", description: "The track's title" },
    "spotify.artist": { kind: "text", description: "Its artists" },
    "spotify.liked": { kind: "bool", description: "Whether you have liked it" },
    "spotify.device": { kind: "text", description: "The device playing" },
    "spotify.volume": { kind: "number", unit: "%", description: "Spotify's volume on that device" },
    "spotify.art": { kind: "text", description: "The album art, for a widget's icon" },
  },

  actions: {
    connect: {
      description: "Sign in to Spotify (opens your browser)",
      run: async (_, ctx) => {
        await connect(ctx);
      },
    },
    like_current: {
      description: "Like the song that is playing",
      run: async (_, ctx) => {
        const track = await current(ctx);
        await call(ctx, "PUT", `/v1/me/tracks?ids=${track.id}`);
        ctx.vars.set({ "spotify.liked": true });
        return { state: "liked" };
      },
    },
    unlike_current: {
      description: "Remove the song that is playing from your liked songs",
      run: async (_, ctx) => {
        const track = await current(ctx);
        await call(ctx, "DELETE", `/v1/me/tracks?ids=${track.id}`);
        ctx.vars.set({ "spotify.liked": false });
        return { state: "unliked" };
      },
    },
    toggle_like: {
      description: "Like the song that is playing, or unlike it",
      run: async (_, ctx) => {
        const track = await current(ctx);
        const liked = await isLiked(ctx, track.id);
        await call(ctx, liked ? "DELETE" : "PUT", `/v1/me/tracks?ids=${track.id}`);
        ctx.vars.set({ "spotify.liked": !liked });
        return { state: liked ? "unliked" : "liked" };
      },
    },
    add_to_playlist: {
      description: "Add the song that is playing to a playlist",
      arguments: {
        playlist: { kind: "text", description: "The playlist", example: "37i9dQZF1DXcBWIGoYBM5M", suggest: () => playlistChoices(ctxOf()) },
      },
      run: async ({ playlist }, ctx) => {
        const track = await current(ctx);
        await call(ctx, "POST", `/v1/playlists/${encodeURIComponent(playlist.trim())}/tracks`, { uris: [track.uri] });
      },
    },
    play_uri: {
      description: "Play a song, album, playlist or artist by its Spotify link",
      arguments: { uri: { kind: "text", description: "A Spotify link or URI", example: "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M" } },
      check: ({ uri }) => {
        try {
          playBody(uri);
        } catch (error) {
          return (error as Error).message;
        }
      },
      run: async ({ uri }, ctx) => {
        await call(ctx, "PUT", "/v1/me/player/play", playBody(uri));
      },
    },
    play_playlist: {
      description: "Play one of your playlists",
      arguments: {
        playlist: { kind: "text", description: "The playlist", example: "37i9dQZF1DXcBWIGoYBM5M", suggest: () => playlistChoices(ctxOf()) },
      },
      run: async ({ playlist }, ctx) => {
        await call(ctx, "PUT", "/v1/me/player/play", { context_uri: `spotify:playlist:${playlist.trim()}` });
      },
    },
    set_volume: {
      description: "Set Spotify's volume (Premium)",
      arguments: { volume: { kind: "number", description: "0 to 100", example: "50" } },
      slider: { argument: "volume", feedback: "spotify.volume" },
      run: async ({ volume }, ctx) => {
        const level = Math.round(Number(volume));
        if (!Number.isFinite(level) || level < 0 || level > 100) throw new Error("the volume must be 0 to 100");
        await call(ctx, "PUT", `/v1/me/player/volume?volume_percent=${level}`);
        ctx.vars.set({ "spotify.volume": level });
      },
    },
    transfer_playback: {
      description: "Move playback to another device",
      arguments: { device: { kind: "text", description: "The device", example: "living-room", suggest: () => deviceChoices(ctxOf()) } },
      run: async ({ device }, ctx) => {
        await call(ctx, "PUT", "/v1/me/player", { device_ids: [device.trim()], play: true });
      },
    },
  },

  sample: async (wanted, ctx) => {
    if (!ctx.settings.refresh_token && !state.accessToken) return { "spotify.connected": false };
    await poll(ctx, wanted.includes("spotify.liked"));
  },

  start: async (ctx) => {
    contextHolder = ctx;
    ctx.vars.set({ "spotify.connected": false });
    if (!ctx.settings.refresh_token) {
      ctx.log.info("not signed in yet: press a Spotify button set to 'Connect'");
      return;
    }
    try {
      await welcome(ctx);
    } catch (error) {
      ctx.log.warn(`could not sign in with the saved token: ${String(error)}`);
    }
  },

  settingsChanged: (ctx) => {
    contextHolder = ctx;
    reset();
  },
});

export default plugin;
