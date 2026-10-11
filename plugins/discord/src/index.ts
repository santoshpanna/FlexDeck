/**
 * Discord for flexdeck.
 *
 * Mute, deafen, push-to-talk, the voice mode, the microphone and speaker and
 * their volumes, each person's volume in the call, joining and leaving voice
 * channels, opening a text channel, the soundboard, video and screen share —
 * and on the deck: whether you are muted, who is in the call and who is
 * speaking, how many notifications came in, and a few numbers about a server.
 *
 * It talks to the Discord desktop app on this computer through its local RPC
 * pipe, so Discord must be running and signed in. The first time, Discord
 * shows a dialog asking whether to let flexdeck control it; after that the
 * plugin keeps its sign-in fresh by itself.
 *
 * # Whose Discord application
 *
 * Local RPC needs a Discord *application* to sign in as, and Discord only lets
 * an application use voice control once it has approved it — or for the people
 * listed as its testers. Until flexdeck has an approved application of its
 * own, you use yours: the README walks through creating one (two minutes), and
 * its Client ID and Secret go in this plugin's settings, on your computer only.
 *
 * # What Discord does not document
 *
 * Toggling video and screen share and playing a soundboard sound are commands
 * Discord's own hardware control-surface integrations use but has not published. They are
 * tried, never assumed: if this Discord does not offer one, the action says so
 * plainly instead of failing obscurely.
 */

import { definePlugin, type Context, type VariableSpec } from "@flexdeck/plugin-sdk";

import { DiscordError, DiscordIpc } from "./ipc.ts";
import { exchange, OAuthError, pkce, refresh, type Credentials, type Tokens } from "./oauth.ts";

const ID = "discord";

/**
 * flexdeck's own Discord application, once Discord has approved it for RPC.
 * Empty until then, which means "use the one in the settings". It is a public
 * application: no secret is ever shipped, and sign-in uses PKCE.
 */
export const OFFICIAL_CLIENT_ID = "";

const SCOPES = ["rpc", "rpc.voice.read", "rpc.voice.write", "rpc.notifications.read", "identify", "guilds"];

/** How long the consent dialog may sit waiting for a person. */
const AUTHORIZE_TIMEOUT_MS = 120_000;

/** Server stats are read at most this often, and only while something shows them. */
const STATS_EVERY_MS = 30_000;

interface Member {
  id: string;
  name: string;
  volume: number;
  muted: boolean;
}

interface Device {
  id: string;
  name: string;
}

interface State {
  ipc: DiscordIpc | null;
  reason: string;
  retry: ReturnType<typeof setTimeout> | null;
  attempt: number;
  stopping: boolean;
  inputDevices: Device[];
  outputDevices: Device[];
  voice: { mute: boolean; deaf: boolean; mode: string };
  channel: { id: string; guildId: string; name: string } | null;
  members: Map<string, Member>;
  notifications: number;
  /** `undefined`: not tried yet. */
  features: { soundboard?: boolean; video?: boolean; screenshare?: boolean };
  statsAt: number;
}

const fresh = (): State => ({
  ipc: null,
  reason: "not started",
  retry: null,
  attempt: 0,
  stopping: false,
  inputDevices: [],
  outputDevices: [],
  voice: { mute: false, deaf: false, mode: "VOICE_ACTIVITY" },
  channel: null,
  members: new Map(),
  notifications: 0,
  features: {},
  statsAt: 0,
});

const state: State = fresh();

/** The pipe to use instead of discord-ipc-0..9: the tests' fake Discord. */
let pipeOverride: string | undefined = process.env.FLEXDECK_DISCORD_PIPE;
export function usePipe(path: string | undefined): void {
  pipeOverride = path;
}

function ipc(): DiscordIpc {
  if (!state.ipc?.connected) throw new Error(`Discord is not connected (${state.reason})`);
  return state.ipc;
}

/**
 * Run a command, turning Discord's refusals into sentences a person can act on.
 * `feature` names an undocumented command, so "unknown command" can be said as
 * "this Discord does not offer that".
 */
async function command(
  cmd: string,
  args: Record<string, unknown> = {},
  feature?: keyof State["features"],
): Promise<Record<string, unknown>> {
  try {
    const answer = await ipc().command(cmd, args);
    if (feature) state.features[feature] = true;
    return answer;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    const code = error instanceof DiscordError ? error.code : undefined;
    if (feature && (code === 4002 || /invalid command|unknown command|not.*supported/i.test(message))) {
      state.features[feature] = false;
      throw new Error("this Discord does not let apps do that (the command is not one Discord has made public)");
    }
    if (code === 4006 || /scope|not authenticated/i.test(message)) {
      throw new Error(
        "Discord did not allow this application to do that — an application needs Discord's RPC approval, " +
          "or you listed as one of its testers (see the README)",
      );
    }
    throw new Error(message);
  }
}

function credentials(ctx: Context): Credentials {
  return {
    clientId: ctx.settings.client_id || OFFICIAL_CLIENT_ID,
    clientSecret: ctx.settings.client_secret ?? "",
    apiBase: ctx.settings.api_base || "https://discord.com/api",
    redirectUri: ctx.settings.redirect_uri ?? "",
  };
}

function keep(ctx: Context, tokens: Tokens): void {
  ctx.storeSettings({
    access_token: tokens.accessToken,
    refresh_token: tokens.refreshToken,
    expires_at: String(tokens.expiresAt),
  });
}

/** Sign in: the stored token, else a refreshed one, else the consent dialog. */
async function authenticate(ctx: Context, link: DiscordIpc): Promise<void> {
  const creds = credentials(ctx);
  const tryToken = async (token: string) => {
    await link.command("AUTHENTICATE", { access_token: token });
  };

  const stored = ctx.settings.access_token;
  const expiresAt = Number(ctx.settings.expires_at || 0);
  if (stored && expiresAt > Date.now() + 60_000) {
    try {
      await tryToken(stored);
      return;
    } catch {
      // Revoked, or Discord forgot it: refresh, or ask again.
    }
  }
  if (ctx.settings.refresh_token) {
    try {
      const tokens = await refresh(creds, ctx.settings.refresh_token);
      keep(ctx, tokens);
      await tryToken(tokens.accessToken);
      return;
    } catch (error) {
      ctx.log.info(`the saved sign-in could not be renewed (${String(error)}); asking again`);
    }
  }

  const extra = (ctx.settings.extra_scopes ?? "").split(/[\s,]+/).filter(Boolean);
  const challenge = creds.clientSecret ? undefined : pkce();
  const args: Record<string, unknown> = { client_id: creds.clientId, scopes: [...SCOPES, ...extra] };
  if (challenge) {
    args.code_challenge = challenge.challenge;
    args.code_challenge_method = "S256";
  }
  ctx.log.info("asking Discord for permission — look for the dialog in the Discord app");
  let code: string;
  try {
    const answer = await link.command("AUTHORIZE", args, AUTHORIZE_TIMEOUT_MS);
    code = String(answer.code ?? "");
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new DiscordError(
      /cancel|denied|closed/i.test(message)
        ? "permission was not given in Discord; press an action to ask again"
        : `Discord would not ask for permission: ${message}`,
    );
  }
  const tokens = await exchange(creds, code, challenge?.verifier);
  keep(ctx, tokens);
  await tryToken(tokens.accessToken);
}

// --- what Discord tells us ------------------------------------------------

function deviceName(devices: Device[], id: unknown): string {
  const wanted = String(id ?? "");
  return devices.find((device) => device.id === wanted)?.name ?? (wanted === "default" ? "Default" : wanted);
}

function readDevices(raw: unknown): Device[] {
  return ((raw as { id?: unknown; name?: unknown }[]) ?? []).map((device) => ({
    id: String(device.id ?? ""),
    name: String(device.name ?? ""),
  }));
}

/** Remember the devices a voice-settings answer lists. */
function learnDevices(data: Record<string, unknown>): void {
  const input = (data.input as Record<string, unknown>) ?? {};
  const output = (data.output as Record<string, unknown>) ?? {};
  if (input.available_devices) state.inputDevices = readDevices(input.available_devices);
  if (output.available_devices) state.outputDevices = readDevices(output.available_devices);
}

function applyVoice(ctx: Context, data: Record<string, unknown>): void {
  const input = (data.input as Record<string, unknown>) ?? {};
  const output = (data.output as Record<string, unknown>) ?? {};
  const mode = (data.mode as Record<string, unknown>) ?? {};
  learnDevices(data);
  const values: Record<string, string | number | boolean> = {};
  if (typeof data.mute === "boolean") values["discord.voice.mute"] = state.voice.mute = data.mute;
  if (typeof data.deaf === "boolean") values["discord.voice.deaf"] = state.voice.deaf = data.deaf;
  if (typeof mode.type === "string") values["discord.voice.mode"] = state.voice.mode = mode.type;
  if (typeof input.volume === "number") values["discord.voice.input_volume"] = Math.round(input.volume);
  if (typeof output.volume === "number") values["discord.voice.output_volume"] = Math.round(output.volume);
  if (input.device_id !== undefined) values["discord.voice.input_device"] = deviceName(state.inputDevices, input.device_id);
  if (output.device_id !== undefined) values["discord.voice.output_device"] = deviceName(state.outputDevices, output.device_id);
  ctx.vars.set(values);
}

const memberName = (data: Record<string, unknown>): string => {
  const user = (data.user as Record<string, unknown>) ?? {};
  return String(data.nick || user.global_name || user.username || user.id || "someone");
};

function readMember(data: Record<string, unknown>): Member {
  const user = (data.user as Record<string, unknown>) ?? {};
  return {
    id: String(user.id ?? ""),
    name: memberName(data),
    volume: Math.round(Number(data.volume ?? 100)),
    muted: Boolean(data.mute),
  };
}

const userVariable = (id: string, what: "volume" | "speaking" | "muted") => `discord.user.${id}.${what}`;

function publishMembers(ctx: Context): void {
  const declared: Record<string, VariableSpec> = {};
  const values: Record<string, number | boolean> = {};
  for (const member of state.members.values()) {
    declared[userVariable(member.id, "volume")] = { kind: "number", unit: "%", description: `${member.name}'s volume` };
    declared[userVariable(member.id, "muted")] = { kind: "bool", description: `Whether ${member.name} is muted for you` };
    declared[userVariable(member.id, "speaking")] = { kind: "bool", description: `Whether ${member.name} is speaking` };
    values[userVariable(member.id, "volume")] = member.volume;
    values[userVariable(member.id, "muted")] = member.muted;
  }
  ctx.vars.declare(declared, { replace: true });
  ctx.vars.set({ ...values, "discord.voice.users": state.members.size });
}

const CHANNEL_EVENTS = ["VOICE_STATE_CREATE", "VOICE_STATE_UPDATE", "VOICE_STATE_DELETE", "SPEAKING_START", "SPEAKING_STOP"];

async function applyChannel(ctx: Context, channel: Record<string, unknown> | null): Promise<void> {
  const link = state.ipc;
  if (!link) return;
  const previous = state.channel;
  if (previous) {
    for (const event of CHANNEL_EVENTS) await link.unsubscribe(event, { channel_id: previous.id }).catch(() => {});
  }
  state.members.clear();
  if (!channel?.id) {
    state.channel = null;
    ctx.vars.set({ "discord.voice.channel": "", "discord.voice.channel_id": "", "discord.voice.guild": "" });
    publishMembers(ctx);
    return;
  }
  state.channel = { id: String(channel.id), guildId: String(channel.guild_id ?? ""), name: String(channel.name ?? "") };
  for (const voiceState of (channel.voice_states as Record<string, unknown>[]) ?? []) {
    const member = readMember(voiceState);
    if (member.id) state.members.set(member.id, member);
  }
  let guild = "";
  if (state.channel.guildId) {
    try {
      guild = String((await link.command("GET_GUILD", { guild_id: state.channel.guildId })).name ?? "");
    } catch {
      // A DM call has no server.
    }
  }
  ctx.vars.set({
    "discord.voice.channel": state.channel.name,
    "discord.voice.channel_id": state.channel.id,
    "discord.voice.guild": guild,
  });
  publishMembers(ctx);
  for (const event of CHANNEL_EVENTS) {
    await link.subscribe(event, { channel_id: state.channel.id }).catch((error) =>
      ctx.log.warn(`could not follow ${event}: ${String(error)}`),
    );
  }
}

function onEvent(ctx: Context, event: string, data: Record<string, unknown>): void {
  switch (event) {
    case "VOICE_SETTINGS_UPDATE":
      applyVoice(ctx, data);
      break;
    case "VOICE_CHANNEL_SELECT": {
      const channelId = data.channel_id ? String(data.channel_id) : "";
      ctx.emit("voice_channel_changed", { channel_id: channelId, guild_id: String(data.guild_id ?? "") });
      if (!channelId) {
        void applyChannel(ctx, null);
      } else {
        void command("GET_CHANNEL", { channel_id: channelId })
          .then((channel) => applyChannel(ctx, channel))
          .catch((error) => ctx.log.warn(`could not read the new channel: ${String(error)}`));
      }
      break;
    }
    case "VOICE_CONNECTION_STATUS":
      ctx.vars.set({ "discord.voice.connection": String(data.state ?? "") });
      break;
    case "NOTIFICATION_CREATE": {
      state.notifications += 1;
      const title = String(data.title ?? "");
      const body = String(data.body ?? "");
      ctx.vars.set({
        "discord.notifications.count": state.notifications,
        "discord.notifications.last_title": title,
        "discord.notifications.last_body": body,
      });
      ctx.emit("notification", { channel_id: String(data.channel_id ?? ""), title, body });
      break;
    }
    case "VOICE_STATE_CREATE":
    case "VOICE_STATE_UPDATE": {
      const member = readMember(data);
      if (!member.id) break;
      const joined = !state.members.has(member.id);
      state.members.set(member.id, member);
      publishMembers(ctx);
      if (joined && event === "VOICE_STATE_CREATE") ctx.emit("user_joined", { user_id: member.id, name: member.name });
      break;
    }
    case "VOICE_STATE_DELETE": {
      const member = readMember(data);
      if (state.members.delete(member.id)) {
        publishMembers(ctx);
        ctx.emit("user_left", { user_id: member.id, name: member.name });
      }
      break;
    }
    case "SPEAKING_START":
    case "SPEAKING_STOP": {
      const id = String(data.user_id ?? "");
      const speaking = event === "SPEAKING_START";
      if (id && id === state.ipc?.user?.id) ctx.vars.set({ "discord.speaking": speaking });
      else if (state.members.has(id)) ctx.vars.set({ [userVariable(id, "speaking")]: speaking });
      ctx.emit(speaking ? "speaking_started" : "speaking_stopped", { user_id: id });
      break;
    }
  }
}

// --- the connection -------------------------------------------------------

function disconnected(ctx: Context, reason: string): void {
  const was = state.ipc !== null;
  state.ipc?.close();
  state.ipc = null;
  state.reason = reason;
  state.channel = null;
  state.members.clear();
  ctx.vars.set({ "discord.connected": false, "discord.authorized": false });
  if (was) ctx.emit("disconnected", { reason });
  schedule(ctx);
}

function schedule(ctx: Context): void {
  if (state.stopping || state.retry) return;
  const delay = Math.min(30_000, 1000 * 2 ** Math.min(state.attempt, 5));
  state.attempt += 1;
  state.retry = setTimeout(() => {
    state.retry = null;
    void connect(ctx);
  }, delay);
}

export async function connect(ctx: Context): Promise<void> {
  if (state.stopping || state.ipc) return;
  const creds = credentials(ctx);
  if (!creds.clientId) {
    state.reason = "set your Discord application's Client ID in this plugin's settings (see the README)";
    ctx.vars.set({ "discord.connected": false, "discord.authorized": false });
    ctx.log.warn(state.reason);
    return;
  }
  const link = new DiscordIpc({ clientId: creds.clientId, path: pipeOverride });
  try {
    await link.open();
  } catch (error) {
    state.reason = error instanceof Error ? error.message : String(error);
    ctx.vars.set({ "discord.connected": false });
    schedule(ctx);
    return;
  }
  state.ipc = link;
  ctx.vars.set({ "discord.connected": true, "discord.user": link.user?.global_name || link.user?.username || "" });
  try {
    await authenticate(ctx, link);
  } catch (error) {
    state.reason = error instanceof Error ? error.message : String(error);
    ctx.vars.set({ "discord.authorized": false });
    ctx.log.warn(state.reason);
    // Not retried on a timer: asking again would put the dialog back up
    // every few seconds. The next action, or new settings, asks again.
    link.close();
    state.ipc = null;
    return;
  }
  state.attempt = 0;
  state.reason = "connected";
  link.on((event, data) => onEvent(ctx, event, data));
  link.onClose((reason) => disconnected(ctx, reason));
  ctx.vars.set({ "discord.authorized": true });
  for (const event of ["VOICE_SETTINGS_UPDATE", "VOICE_CHANNEL_SELECT", "VOICE_CONNECTION_STATUS", "NOTIFICATION_CREATE"]) {
    await link.subscribe(event).catch((error) => ctx.log.warn(`could not follow ${event}: ${String(error)}`));
  }
  try {
    applyVoice(ctx, await link.command("GET_VOICE_SETTINGS"));
    const channel = await link.command("GET_SELECTED_VOICE_CHANNEL").catch(() => null);
    await applyChannel(ctx, channel && channel.id ? channel : null);
  } catch (error) {
    ctx.log.error(`signed in, but could not read Discord's voice state: ${String(error)}`);
  }
  ctx.emit("connected", {});
  ctx.log.info(`connected to Discord as ${link.user?.username ?? "you"}`);
}

/** Connect now if not connected — an action pressed while signed out asks again. */
async function ensure(ctx: Context): Promise<void> {
  if (!state.ipc?.connected) await connect(ctx);
  ipc();
}

export function disconnect(): void {
  state.stopping = true;
  if (state.retry) clearTimeout(state.retry);
  state.retry = null;
  state.ipc?.close();
  state.ipc = null;
}

/** For the tests: forget everything between cases. */
export function reset(): void {
  disconnect();
  Object.assign(state, fresh());
}

// --- suggestions ---------------------------------------------------------

async function channelChoices(type: number): Promise<{ value: string; label: string }[]> {
  const { guilds } = (await command("GET_GUILDS")) as { guilds?: { id: string; name: string }[] };
  const found: { value: string; label: string }[] = [];
  for (const guild of (guilds ?? []).slice(0, 25)) {
    try {
      const { channels } = (await command("GET_CHANNELS", { guild_id: guild.id })) as {
        channels?: { id: string; name: string; type: number }[];
      };
      for (const channel of channels ?? []) {
        if (channel.type === type) found.push({ value: channel.id, label: `${guild.name} / ${channel.name}` });
      }
    } catch {
      // A server whose channels cannot be read is left out.
    }
  }
  return found;
}

async function deviceChoices(which: "input" | "output"): Promise<{ value: string; label: string }[]> {
  learnDevices(await command("GET_VOICE_SETTINGS"));
  const devices = which === "input" ? state.inputDevices : state.outputDevices;
  return devices.map((device) => ({ value: device.id, label: device.name }));
}

async function userChoices(): Promise<{ value: string; label: string }[]> {
  ipc();
  return [...state.members.values()].map((member) => ({ value: member.id, label: member.name }));
}

async function soundChoices(): Promise<{ value: string; label: string }[]> {
  const answer = await command("GET_SOUNDBOARD_SOUNDS", {}, "soundboard");
  const sounds = ((answer.sounds ?? answer) as Record<string, unknown>[]) ?? [];
  return (Array.isArray(sounds) ? sounds : []).map((sound) => ({
    value: `${String(sound.guild_id ?? "0")}:${String(sound.sound_id ?? sound.id ?? "")}`,
    label: String(sound.name ?? sound.sound_id ?? "sound"),
  }));
}

/** A device the person means: by id, or by (part of) its name. */
function pickDevice(devices: Device[], wanted: string): Device {
  const text = wanted.trim().toLowerCase();
  const found =
    devices.find((device) => device.id === wanted.trim()) ??
    devices.find((device) => device.name.toLowerCase() === text) ??
    (() => {
      const matches = devices.filter((device) => device.name.toLowerCase().includes(text));
      return matches.length === 1 ? matches[0] : undefined;
    })();
  if (!found) {
    throw new Error(`no single Discord device matches '${wanted}' (there are: ${devices.map((d) => d.name).join(", ")})`);
  }
  return found;
}

function percent(raw: string, max: number): number {
  const value = Number(raw);
  if (!Number.isFinite(value) || value < 0 || value > max) throw new Error(`the volume must be 0 to ${max}`);
  return Math.round(value);
}

const MUTED = { kind: "choice" as const, description: "On or off", example: "true", choices: ["true", "false"] };

const plugin = definePlugin({
  id: ID,
  name: "Discord",
  version: "0.1.0",
  description:
    "Mute, deafen, push-to-talk, voice channels, volumes, the soundboard and more in the Discord app — and who is talking, on the deck.",
  author: "flexdeck",
  permissions: ["network"],

  settings: {
    client_id: {
      kind: "text",
      description: "Your Discord application's Client ID (see the README)",
      default: "",
    },
    client_secret: { kind: "text", description: "Your Discord application's Client Secret", secret: true },
    redirect_uri: {
      kind: "text",
      description: "A redirect your application lists (Developer Portal → OAuth2)",
      default: "http://127.0.0.1",
    },
    extra_scopes: {
      kind: "text",
      description: "More permissions to ask for, separated by spaces (for video and screen share, if your app is allowed them)",
      default: "",
    },
    stats_guild: {
      kind: "text",
      description: "The server the stats describe: its id or name (blank: the one you are in a call on)",
      default: "",
    },
    use_guild_widget: {
      kind: "choice",
      description: "Read how many are online from the server's public widget (it must be switched on)",
      choices: ["false", "true"],
      default: "false",
    },
    access_token: { kind: "text", description: "Kept by the plugin; clear it to sign out", secret: true },
    refresh_token: { kind: "text", description: "Kept by the plugin", secret: true },
    expires_at: { kind: "text", description: "Kept by the plugin", default: "" },
    api_base: { kind: "text", description: "Discord's API (leave as it is)", default: "https://discord.com/api" },
  },

  variables: {
    "discord.connected": { kind: "bool", description: "Whether the Discord app is reachable" },
    "discord.authorized": { kind: "bool", description: "Whether Discord let flexdeck in" },
    "discord.user": { kind: "text", description: "Who is signed in to Discord" },
    "discord.voice.mute": { kind: "bool", description: "Whether your microphone is muted" },
    "discord.voice.deaf": { kind: "bool", description: "Whether you are deafened" },
    "discord.voice.mode": {
      kind: "choice",
      description: "Voice activity or push-to-talk",
      choices: ["VOICE_ACTIVITY", "PUSH_TO_TALK"],
    },
    "discord.voice.input_volume": { kind: "number", unit: "%", description: "Your microphone's volume in Discord" },
    "discord.voice.output_volume": { kind: "number", unit: "%", description: "Discord's output volume" },
    "discord.voice.input_device": { kind: "text", description: "The microphone Discord uses" },
    "discord.voice.output_device": { kind: "text", description: "The speakers Discord uses" },
    "discord.voice.channel": { kind: "text", description: "The voice channel you are in" },
    "discord.voice.channel_id": { kind: "text", description: "Its id" },
    "discord.voice.guild": { kind: "text", description: "The server it is on" },
    "discord.voice.users": { kind: "number", description: "How many are in the call" },
    "discord.voice.connection": { kind: "text", description: "The voice connection's state" },
    "discord.speaking": { kind: "bool", description: "Whether you are speaking" },
    "discord.notifications.count": { kind: "number", description: "Notifications since they were last cleared" },
    "discord.notifications.last_title": { kind: "text", description: "The latest notification's title" },
    "discord.notifications.last_body": { kind: "text", description: "The latest notification's text" },
    "discord.stats.guild_name": { kind: "text", description: "The stats server's name" },
    "discord.stats.voice_channels": { kind: "number", description: "How many voice channels it has" },
    "discord.stats.in_voice": { kind: "number", description: "How many people are in its voice channels" },
    "discord.stats.online": { kind: "number", description: "How many are online (needs the server's widget)" },
  },

  events: {
    connected: { description: "Discord connected" },
    disconnected: { description: "Discord went away", fields: ["reason"] },
    notification: { description: "A notification arrived", fields: ["channel_id", "title", "body"] },
    voice_channel_changed: { description: "You joined, left or moved voice channel", fields: ["channel_id", "guild_id"] },
    user_joined: { description: "Someone joined your call", fields: ["user_id", "name"] },
    user_left: { description: "Someone left your call", fields: ["user_id", "name"] },
    speaking_started: { description: "Someone started speaking", fields: ["user_id"] },
    speaking_stopped: { description: "Someone stopped speaking", fields: ["user_id"] },
  },

  actions: {
    toggle_mute: {
      description: "Mute or unmute your microphone",
      run: async (_, ctx) => {
        await ensure(ctx);
        const mute = !state.voice.mute;
        await command("SET_VOICE_SETTINGS", { mute });
        return { state: mute ? "muted" : "live" };
      },
    },
    set_mute: {
      description: "Set your microphone muted or not",
      arguments: { muted: MUTED },
      run: async ({ muted }, ctx) => {
        await ensure(ctx);
        await command("SET_VOICE_SETTINGS", { mute: muted === "true" });
      },
    },
    toggle_deafen: {
      description: "Deafen or undeafen",
      run: async (_, ctx) => {
        await ensure(ctx);
        const deaf = !state.voice.deaf;
        await command("SET_VOICE_SETTINGS", { deaf });
        return { state: deaf ? "deafened" : "hearing" };
      },
    },
    set_deafen: {
      description: "Set deafened or not",
      arguments: { deaf: { ...MUTED, description: "Deafened or not" } },
      run: async ({ deaf }, ctx) => {
        await ensure(ctx);
        await command("SET_VOICE_SETTINGS", { deaf: deaf === "true" });
      },
    },
    ptt_start: {
      description: "Push-to-talk: open the microphone (put on a button's 'when pressed')",
      run: async (_, ctx) => {
        await ensure(ctx);
        await command("SET_VOICE_SETTINGS", { mute: false });
      },
    },
    ptt_end: {
      description: "Push-to-talk: close the microphone (put on 'when let go')",
      run: async (_, ctx) => {
        await ensure(ctx);
        await command("SET_VOICE_SETTINGS", { mute: true });
      },
    },
    toggle_voice_mode: {
      description: "Switch between voice activity and push-to-talk",
      run: async (_, ctx) => {
        await ensure(ctx);
        const type = state.voice.mode === "PUSH_TO_TALK" ? "VOICE_ACTIVITY" : "PUSH_TO_TALK";
        await command("SET_VOICE_SETTINGS", { mode: { type } });
        return { state: type === "PUSH_TO_TALK" ? "ptt" : "vad" };
      },
    },
    set_voice_mode: {
      description: "Use voice activity or push-to-talk",
      arguments: {
        mode: { kind: "choice", description: "The input mode", example: "PUSH_TO_TALK", choices: ["VOICE_ACTIVITY", "PUSH_TO_TALK"] },
      },
      run: async ({ mode }, ctx) => {
        await ensure(ctx);
        await command("SET_VOICE_SETTINGS", { mode: { type: mode } });
      },
    },
    join_voice: {
      description: "Join a voice channel",
      arguments: {
        channel: { kind: "text", description: "The voice channel", example: "123456789012345678", suggest: () => channelChoices(2) },
        force: {
          kind: "choice",
          description: "Move even if you are in another call",
          example: "false",
          choices: ["false", "true"],
          optional: true,
        },
      },
      run: async ({ channel, force }, ctx) => {
        await ensure(ctx);
        await command("SELECT_VOICE_CHANNEL", { channel_id: channel.trim(), force: force === "true" });
      },
    },
    leave_voice: {
      description: "Leave the voice channel",
      run: async (_, ctx) => {
        await ensure(ctx);
        await command("SELECT_VOICE_CHANNEL", { channel_id: null });
      },
    },
    select_text_channel: {
      description: "Open a text channel in Discord",
      arguments: {
        channel: { kind: "text", description: "The text channel", example: "123456789012345678", suggest: () => channelChoices(0) },
      },
      run: async ({ channel }, ctx) => {
        await ensure(ctx);
        await command("SELECT_TEXT_CHANNEL", { channel_id: channel.trim() });
      },
    },
    set_input_device: {
      description: "Choose Discord's microphone",
      arguments: { device: { kind: "text", description: "The microphone", example: "default", suggest: () => deviceChoices("input") } },
      run: async ({ device }, ctx) => {
        await ensure(ctx);
        applyVoice(ctx, await command("GET_VOICE_SETTINGS"));
        await command("SET_VOICE_SETTINGS", { input: { device_id: pickDevice(state.inputDevices, device).id } });
      },
    },
    set_output_device: {
      description: "Choose Discord's speakers",
      arguments: { device: { kind: "text", description: "The speakers", example: "default", suggest: () => deviceChoices("output") } },
      run: async ({ device }, ctx) => {
        await ensure(ctx);
        applyVoice(ctx, await command("GET_VOICE_SETTINGS"));
        await command("SET_VOICE_SETTINGS", { output: { device_id: pickDevice(state.outputDevices, device).id } });
      },
    },
    set_input_volume: {
      description: "Set your microphone's volume in Discord",
      arguments: { volume: { kind: "number", description: "0 to 100", example: "100" } },
      slider: { argument: "volume", min: 0, max: 100, feedback: "discord.voice.input_volume" },
      check: ({ volume }) => {
        try {
          percent(volume, 100);
        } catch (error) {
          return (error as Error).message;
        }
      },
      run: async ({ volume }, ctx) => {
        await ensure(ctx);
        await command("SET_VOICE_SETTINGS", { input: { volume: percent(volume, 100) } });
      },
    },
    set_output_volume: {
      description: "Set Discord's output volume",
      arguments: { volume: { kind: "number", description: "0 to 200", example: "100" } },
      slider: { argument: "volume", min: 0, max: 200, feedback: "discord.voice.output_volume" },
      run: async ({ volume }, ctx) => {
        await ensure(ctx);
        await command("SET_VOICE_SETTINGS", { output: { volume: percent(volume, 200) } });
      },
    },
    set_user_volume: {
      description: "Set one person's volume in your call",
      arguments: {
        user: { kind: "text", description: "Who", example: "123456789012345678", suggest: () => userChoices() },
        volume: { kind: "number", description: "0 to 200", example: "100" },
      },
      slider: { argument: "volume", min: 0, max: 200, feedback: "discord.user.{{user}}.volume" },
      run: async ({ user, volume }, ctx) => {
        await ensure(ctx);
        const level = percent(volume, 200);
        await command("SET_USER_VOICE_SETTINGS", { user_id: user.trim(), volume: level });
        const member = state.members.get(user.trim());
        if (member) {
          member.volume = level;
          ctx.vars.set({ [userVariable(member.id, "volume")]: level });
        }
      },
    },
    toggle_user_mute: {
      description: "Mute or unmute one person for yourself",
      arguments: { user: { kind: "text", description: "Who", example: "123456789012345678", suggest: () => userChoices() } },
      run: async ({ user }, ctx) => {
        await ensure(ctx);
        const member = state.members.get(user.trim());
        const mute = !(member?.muted ?? false);
        await command("SET_USER_VOICE_SETTINGS", { user_id: user.trim(), mute });
        if (member) {
          member.muted = mute;
          ctx.vars.set({ [userVariable(member.id, "muted")]: mute });
        }
        return { state: mute ? "muted" : "live" };
      },
    },
    play_sound: {
      description: "Play a soundboard sound in your call",
      arguments: { sound: { kind: "text", description: "The sound", example: "0:123456789012345678", suggest: () => soundChoices() } },
      check: () => (state.features.soundboard === false ? "this Discord does not let apps play soundboard sounds" : undefined),
      run: async ({ sound }, ctx) => {
        await ensure(ctx);
        const [guildId, soundId] = sound.includes(":") ? sound.split(":", 2) : ["0", sound];
        await command(
          "PLAY_SOUNDBOARD_SOUND",
          { guild_id: guildId === "0" ? undefined : guildId, sound_id: soundId?.trim() },
          "soundboard",
        );
      },
    },
    toggle_video: {
      description: "Turn your camera on or off in the call",
      check: () => (state.features.video === false ? "this Discord does not let apps turn the camera on or off" : undefined),
      run: async (_, ctx) => {
        await ensure(ctx);
        await command("TOGGLE_VIDEO", {}, "video");
      },
    },
    toggle_screenshare: {
      description: "Start or stop sharing your screen",
      check: () => (state.features.screenshare === false ? "this Discord does not let apps start a screen share" : undefined),
      run: async (_, ctx) => {
        await ensure(ctx);
        await command("TOGGLE_SCREENSHARE", {}, "screenshare");
      },
    },
    clear_notifications: {
      description: "Reset the notification count",
      run: (_, ctx) => {
        state.notifications = 0;
        ctx.vars.set({ "discord.notifications.count": 0 });
      },
    },
  },

  /**
   * Only the server stats are read on demand — everything else arrives as
   * Discord's events — and at most every 30 seconds, since counting who is in
   * voice takes one request per channel.
   */
  sample: async (wanted, ctx) => {
    if (!state.ipc?.connected) return { "discord.connected": false };
    if (!wanted.some((name) => name.startsWith("discord.stats.")) || Date.now() - state.statsAt < STATS_EVERY_MS) return;
    state.statsAt = Date.now();
    const target = (ctx.settings.stats_guild ?? "").trim();
    let guildId = state.channel?.guildId ?? "";
    let guildName = "";
    if (target) {
      const { guilds } = (await command("GET_GUILDS")) as { guilds?: { id: string; name: string }[] };
      const found = (guilds ?? []).find((guild) => guild.id === target || guild.name.toLowerCase() === target.toLowerCase());
      if (!found) throw new Error(`you are not on a server called '${target}'`);
      guildId = found.id;
      guildName = found.name;
    }
    if (!guildId) return;
    if (!guildName) guildName = String((await command("GET_GUILD", { guild_id: guildId })).name ?? "");
    const { channels } = (await command("GET_CHANNELS", { guild_id: guildId })) as {
      channels?: { id: string; type: number }[];
    };
    const voice = (channels ?? []).filter((channel) => channel.type === 2);
    let inVoice = 0;
    for (const channel of voice.slice(0, 25)) {
      const details = await command("GET_CHANNEL", { channel_id: channel.id }).catch(() => ({}));
      inVoice += ((details as { voice_states?: unknown[] }).voice_states ?? []).length;
    }
    const values: Record<string, string | number> = {
      "discord.stats.guild_name": guildName,
      "discord.stats.voice_channels": voice.length,
      "discord.stats.in_voice": inVoice,
    };
    if (ctx.settings.use_guild_widget === "true") {
      try {
        const response = await fetch(`${credentials(ctx).apiBase}/guilds/${guildId}/widget.json`);
        if (response.ok) values["discord.stats.online"] = Number(((await response.json()) as { presence_count?: number }).presence_count ?? 0);
      } catch {
        // The widget is optional; the other numbers still stand.
      }
    }
    return values;
  },

  start: (ctx) => {
    state.stopping = false;
    ctx.vars.set({ "discord.connected": false, "discord.authorized": false });
    void connect(ctx);
  },

  settingsChanged: (ctx) => {
    reset();
    state.stopping = false;
    void connect(ctx);
  },

  stop: () => disconnect(),
});

export default plugin;

export { OAuthError };
