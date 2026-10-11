/**
 * Streamer.bot for flexdeck.
 *
 * Run Streamer.bot actions from the deck — with arguments, or with a slider's
 * value — flip between two actions with one button, switch actions on and
 * off, and show on the deck whether Streamer.bot is connected, whether an
 * action is enabled, and the value of any global variable.
 *
 * Needs Streamer.bot's WebSocket server started (Servers/Clients → WebSocket
 * Server). The connection is kept open and re-made on its own when
 * Streamer.bot restarts.
 *
 * Streamer.bot's WebSocket API can run an action but cannot switch one on or
 * off, so those three actions run a small helper action you import once (see
 * the README); everything else needs nothing installed in Streamer.bot.
 */

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { definePlugin, type Context, type VariableSpec } from "@flexdeck/plugin-sdk";

import { type Connect, type SbAction, type SbEvent, StreamerbotClient } from "./sb-client.ts";

const ID = "streamerbot";

/** A variable-safe spelling of a name: "Scene: Live!" -> "scene_live". */
export function slug(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "") || "action";
}

/** `key=value;key2=value 2` as arguments. A value may hold `=`; not `;`. */
export function parseArgs(text: string | undefined): Record<string, string> {
  const args: Record<string, string> = {};
  for (const pair of (text ?? "").split(";")) {
    const at = pair.indexOf("=");
    if (at <= 0) continue;
    const key = pair.slice(0, at).trim();
    if (key) args[key] = pair.slice(at + 1).trim();
  }
  return args;
}

const GUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

interface State {
  client: StreamerbotClient | null;
  reason: string;
  actions: SbAction[];
  retry: ReturnType<typeof setTimeout> | null;
  attempt: number;
  stopping: boolean;
  refreshing: ReturnType<typeof setTimeout> | null;
}

const state: State = {
  client: null,
  reason: "not started",
  actions: [],
  retry: null,
  attempt: 0,
  stopping: false,
  refreshing: null,
};

let connectOverride: Connect | undefined;
/** Injected by the tests; the real WebSocket otherwise. */
export function useConnection(connect: Connect | undefined): void {
  connectOverride = connect;
}

function client(): StreamerbotClient {
  if (!state.client?.connected) throw new Error(`Streamer.bot is not connected (${state.reason})`);
  return state.client;
}

/** Turn a client error into the sentence alone. */
async function sb<T>(work: () => Promise<T>): Promise<T> {
  try {
    return await work();
  } catch (error) {
    throw new Error(error instanceof Error ? error.message : String(error));
  }
}

/** The action a person means: by id, by name, or by the slug its variable uses. */
export function findAction(actions: readonly SbAction[], wanted: string): SbAction | undefined {
  const text = wanted.trim();
  return (
    actions.find((action) => action.id === text) ??
    actions.find((action) => action.name === text) ??
    actions.find((action) => action.name.toLowerCase() === text.toLowerCase()) ??
    actions.find((action) => slug(action.name) === slug(text))
  );
}

/** How an action argument is sent: by id when known, else by name. */
function reference(wanted: string): { id?: string; name?: string } {
  const known = findAction(state.actions, wanted);
  if (known) return { id: known.id };
  return GUID.test(wanted.trim()) ? { id: wanted.trim() } : { name: wanted.trim() };
}

const enabledVariable = (action: SbAction) => `streamerbot.action.${slug(action.name)}.enabled`;

/** Declare one `.enabled` variable per action and publish them all. */
function publishActions(ctx: Context): void {
  const declared: Record<string, VariableSpec> = {};
  const values: Record<string, boolean> = {};
  for (const action of state.actions) {
    declared[enabledVariable(action)] = {
      kind: "bool",
      description: `Whether "${action.name}" is enabled`,
    };
    values[enabledVariable(action)] = action.enabled;
  }
  // Switches and globals are declared on their own; keep them.
  for (const variable of plugin.variables()) {
    if (variable.name.startsWith("streamerbot.switch.") || variable.name.startsWith("streamerbot.global.")) {
      declared[variable.name] = { kind: variable.kind, description: variable.description };
    }
  }
  ctx.vars.declare(declared, { replace: true });
  ctx.vars.set(values);
}

async function refreshActions(ctx: Context): Promise<void> {
  state.actions = await client().actions();
  publishActions(ctx);
}

/** Refresh once, a moment after a burst of "action changed" events. */
function refreshSoon(ctx: Context): void {
  if (state.refreshing) return;
  state.refreshing = setTimeout(() => {
    state.refreshing = null;
    void refreshActions(ctx).catch(() => {});
  }, 200);
}

function mirroredGlobals(ctx: Context): string[] {
  return (ctx.settings.globals ?? "")
    .split(",")
    .map((name) => name.trim())
    .filter(Boolean);
}

const globalVariable = (name: string) => `streamerbot.global.${slug(name)}`;

async function refreshGlobals(ctx: Context): Promise<void> {
  const wanted = mirroredGlobals(ctx);
  if (wanted.length === 0) return;
  const all = await client().globals();
  const declared: Record<string, VariableSpec> = {};
  const values: Record<string, string> = {};
  for (const name of wanted) {
    declared[globalVariable(name)] = { kind: "text", description: `Streamer.bot's global "${name}"` };
    if (name in all) values[globalVariable(name)] = String(all[name] ?? "");
  }
  ctx.vars.declare(declared);
  ctx.vars.set(values);
}

function onEvent(ctx: Context, event: SbEvent): void {
  const key = `${event.source}.${event.type}`;
  switch (key) {
    case "Raw.ActionCompleted": {
      const name = String(event.data.actionName ?? event.data.name ?? "");
      const id = String(event.data.actionId ?? event.data.id ?? "");
      ctx.emit("action_completed", { action: name, id });
      break;
    }
    case "Application.ActionAdded":
    case "Application.ActionUpdated":
    case "Application.ActionDeleted":
      refreshSoon(ctx);
      break;
    case "General.Custom":
      ctx.emit("custom", { data: JSON.stringify(event.data) });
      break;
    case "Misc.GlobalVariableUpdated":
    case "Misc.GlobalVariableCreated": {
      const name = String(event.data.name ?? event.data.variableName ?? "");
      const value = String(event.data.newValue ?? event.data.value ?? "");
      if (mirroredGlobals(ctx).includes(name)) ctx.vars.set({ [globalVariable(name)]: value });
      if (name) ctx.emit("global_changed", { name, value });
      break;
    }
  }
}

function disconnected(ctx: Context, reason: string): void {
  const was = state.client !== null;
  state.client?.close();
  state.client = null;
  state.reason = reason;
  ctx.vars.set({ "streamerbot.connected": false });
  if (was) ctx.emit("disconnected", { reason });
  schedule(ctx);
}

/** Try again in 1, 2, 4 … 30 seconds. Streamer.bot restarting is normal. */
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
  if (state.stopping) return;
  const host = ctx.settings.host || "127.0.0.1";
  const port = ctx.settings.port || "8080";
  const endpoint = (ctx.settings.endpoint || "/").startsWith("/") ? ctx.settings.endpoint || "/" : `/${ctx.settings.endpoint}`;
  const bot = new StreamerbotClient({
    url: `ws://${host}:${port}${endpoint}`,
    password: ctx.settings.password,
    connect: connectOverride,
  });
  try {
    await bot.open();
  } catch (error) {
    state.reason = error instanceof Error ? error.message : String(error);
    ctx.vars.set({ "streamerbot.connected": false });
    ctx.log.warn(state.reason);
    schedule(ctx);
    return;
  }
  state.client = bot;
  state.attempt = 0;
  state.reason = "connected";
  bot.on((event) => onEvent(ctx, event));
  bot.onClose((reason) => disconnected(ctx, reason));
  ctx.vars.set({
    "streamerbot.connected": true,
    "streamerbot.version": String(bot.info.version ?? ""),
  });
  try {
    const refused = await bot.subscribe({
      General: ["Custom"],
      Raw: ["ActionCompleted"],
      Application: ["ActionAdded", "ActionUpdated", "ActionDeleted"],
      Misc: ["GlobalVariableUpdated", "GlobalVariableCreated"],
    });
    if (refused.length > 0) ctx.log.info(`this Streamer.bot does not offer ${refused.join(", ")} events`);
    await refreshActions(ctx);
    await refreshGlobals(ctx);
  } catch (error) {
    ctx.log.error(`connected, but could not read Streamer.bot's state: ${String(error)}`);
  }
  ctx.emit("connected", {});
  ctx.log.info(`connected to Streamer.bot at ${host}:${port}`);
}

export function disconnect(): void {
  state.stopping = true;
  if (state.retry) clearTimeout(state.retry);
  if (state.refreshing) clearTimeout(state.refreshing);
  state.retry = null;
  state.refreshing = null;
  state.client?.close();
  state.client = null;
}

/** For the tests: forget everything between cases. */
export function reset(): void {
  disconnect();
  state.stopping = false;
  state.attempt = 0;
  state.reason = "not started";
  state.actions = [];
}

// --- switches: which of two actions runs next ---------------------------

function switchesFile(ctx: Context): string | null {
  return ctx.dataDir ? join(ctx.dataDir, "switches.json") : null;
}

const switches = new Map<string, "a" | "b">();
let switchesLoaded = false;

function loadSwitches(ctx: Context): void {
  if (switchesLoaded) return;
  switchesLoaded = true;
  const file = switchesFile(ctx);
  if (!file) return;
  try {
    const saved = JSON.parse(readFileSync(file, "utf8")) as Record<string, string>;
    for (const [key, side] of Object.entries(saved)) {
      if (side === "a" || side === "b") switches.set(key, side);
    }
  } catch {
    // First run, or a file someone broke by hand: start every switch fresh.
  }
}

function saveSwitches(ctx: Context): void {
  const file = switchesFile(ctx);
  if (!file) return;
  try {
    writeFileSync(file, JSON.stringify(Object.fromEntries(switches)));
  } catch (error) {
    ctx.log.warn(`could not remember the switches: ${String(error)}`);
  }
}

/** For the tests. */
export function forgetSwitches(): void {
  switches.clear();
  switchesLoaded = false;
}

// --- suggestions ---------------------------------------------------------

async function actionChoices(): Promise<{ value: string; label: string }[]> {
  const actions = await sb(() => client().actions());
  state.actions = actions;
  return actions
    .slice()
    .sort((a, b) => `${a.group}/${a.name}`.localeCompare(`${b.group}/${b.name}`))
    .map((action) => ({
      value: action.id,
      label: action.group ? `${action.group} / ${action.name}` : action.name,
    }));
}

const unknownAction = (wanted: string): string | undefined =>
  state.client?.connected && state.actions.length > 0 && !findAction(state.actions, wanted) && !GUID.test(wanted.trim())
    ? `Streamer.bot has no action called '${wanted}'`
    : undefined;

const ACTION = {
  kind: "text" as const,
  description: "The Streamer.bot action",
  example: "Shout-out",
  suggest: () => actionChoices(),
};

async function setActionState(ctx: Context, wanted: string, mode: "enable" | "disable" | "toggle"): Promise<{ state: string }> {
  const target = findAction(state.actions, wanted);
  if (!target) throw new Error(unknownAction(wanted) ?? `Streamer.bot has no action called '${wanted}'`);
  const helper = ctx.settings.helper_action || "flexdeck: set action state";
  const known = findAction(state.actions, helper);
  if (!known) {
    throw new Error(
      `switching actions on and off needs the "${helper}" helper action imported into Streamer.bot (see the plugin's README)`,
    );
  }
  const enabled = mode === "toggle" ? !target.enabled : mode === "enable";
  await sb(() => client().doAction({ id: known.id }, { actionId: target.id, actionName: target.name, enabled }));
  // Shown at once; Streamer.bot's own "action updated" event confirms it.
  target.enabled = enabled;
  ctx.vars.set({ [enabledVariable(target)]: enabled });
  refreshSoon(ctx);
  return { state: enabled ? "enabled" : "disabled" };
}

const plugin = definePlugin({
  id: ID,
  name: "Streamer.bot",
  version: "0.1.0",
  description:
    "Run Streamer.bot actions, flip between two, switch them on and off, and show their state and global variables on the deck.",
  author: "flexdeck",
  permissions: ["network"],

  settings: {
    host: { kind: "text", description: "The computer Streamer.bot runs on", default: "127.0.0.1" },
    port: { kind: "number", description: "Its WebSocket server's port", default: "8080" },
    endpoint: { kind: "text", description: "Its WebSocket server's endpoint", default: "/" },
    password: { kind: "text", description: "The WebSocket server password, if authentication is on", secret: true },
    globals: {
      kind: "text",
      description: "Global variables to show on the deck, separated by commas",
      default: "",
    },
    helper_action: {
      kind: "text",
      description: "The helper action that switches actions on and off (see the README)",
      default: "flexdeck: set action state",
    },
  },

  variables: {
    "streamerbot.connected": { kind: "bool", description: "Whether Streamer.bot is connected" },
    "streamerbot.version": { kind: "text", description: "Streamer.bot's version" },
  },

  events: {
    connected: { description: "Streamer.bot connected" },
    disconnected: { description: "Streamer.bot went away", fields: ["reason"] },
    action_completed: { description: "An action finished running", fields: ["action", "id"] },
    custom: { description: "A custom event was broadcast (General → Custom)", fields: ["data"] },
    global_changed: { description: "A global variable changed", fields: ["name", "value"] },
  },

  actions: {
    run_action: {
      description: "Run a Streamer.bot action",
      arguments: {
        action: ACTION,
        args: {
          kind: "text",
          description: "Arguments, as name=value;name2=value2",
          example: "",
          optional: true,
        },
      },
      check: ({ action }) => unknownAction(action),
      run: async ({ action, args }) => {
        await sb(() => client().doAction(reference(action), parseArgs(args)));
      },
    },

    run_action_value: {
      description: "Run an action with a value — from a slider, say",
      arguments: {
        action: ACTION,
        argument: { kind: "text", description: "The argument the value is passed as", example: "value" },
        value: { kind: "number", description: "The value, 0 to 100", example: "50" },
      },
      slider: { argument: "value", min: 0, max: 100, step: 1 },
      check: ({ action, value }) => {
        const number = Number(value);
        if (!Number.isFinite(number)) return "the value must be a number";
        return unknownAction(action);
      },
      run: async ({ action, argument, value }) => {
        await sb(() => client().doAction(reference(action), { [argument || "value"]: Number(value) }));
      },
    },

    switch: {
      description: "Flip between two actions: the first, then the second, then the first again",
      arguments: {
        key: { kind: "text", description: "A name for this switch, so the deck remembers which is next", example: "lights" },
        action_a: { ...ACTION, description: "The first action", example: "Lights on" },
        action_b: { ...ACTION, description: "The second action", example: "Lights off" },
      },
      check: ({ action_a, action_b }) => unknownAction(action_a) ?? unknownAction(action_b),
      run: async ({ key, action_a, action_b }, ctx) => {
        loadSwitches(ctx);
        const name = slug(key);
        const next = switches.get(name) === "a" ? "b" : "a";
        await sb(() => client().doAction(reference(next === "a" ? action_a : action_b)));
        switches.set(name, next);
        saveSwitches(ctx);
        const variable = `streamerbot.switch.${name}`;
        ctx.vars.declare({ [variable]: { kind: "choice", choices: ["a", "b"], description: `Which side "${key}" is on` } });
        ctx.vars.set({ [variable]: next });
        return { state: next };
      },
    },

    enable_action: {
      description: "Switch an action on (needs the helper action)",
      arguments: { action: ACTION },
      run: ({ action }, ctx) => setActionState(ctx, action, "enable"),
    },
    disable_action: {
      description: "Switch an action off (needs the helper action)",
      arguments: { action: ACTION },
      run: ({ action }, ctx) => setActionState(ctx, action, "disable"),
    },
    toggle_action: {
      description: "Switch an action on if it is off, off if it is on (needs the helper action)",
      arguments: { action: ACTION },
      run: ({ action }, ctx) => setActionState(ctx, action, "toggle"),
    },
  },

  // Event-driven, but a deck watching an action's state or a global is kept
  // honest against a Streamer.bot that does not announce every change.
  sample: async (wanted, ctx) => {
    if (!state.client?.connected) return { "streamerbot.connected": false };
    if (wanted.some((name) => name.startsWith("streamerbot.action."))) await refreshActions(ctx);
    if (wanted.some((name) => name.startsWith("streamerbot.global."))) await refreshGlobals(ctx);
  },

  start: (ctx) => {
    state.stopping = false;
    ctx.vars.set({ "streamerbot.connected": false });
    void connect(ctx);
  },

  settingsChanged: (ctx) => {
    reset();
    void connect(ctx);
  },

  stop: () => disconnect(),
});

export default plugin;
