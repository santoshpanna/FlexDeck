/**
 * OBS Studio for flexdeck.
 *
 * Switch scenes, start and stop recording and streaming, mute inputs, toggle
 * sources — and show what OBS is doing on the deck: the live scene, whether it
 * is recording, for how long, and which microphones are muted.
 *
 * Needs OBS 28 or later with its WebSocket server enabled (Tools → WebSocket
 * Server Settings). The connection is kept open and re-established on its own
 * when OBS restarts, so a deck never has to be told OBS came back.
 *
 * State arrives as OBS *events*, not by polling: a scene change shows up on the
 * deck the moment it happens, and an idle OBS costs nothing. The two timecodes
 * are the exception — OBS does not announce a second ticking by — so they are
 * read only while something on the deck is showing them.
 */

import { definePlugin, type Context, type VariableSpec } from "@flexdeck/plugin-sdk";

import { type Connect, ObsClient } from "./obs-client.ts";

const ID = "obs";

/** A variable-safe spelling of an OBS input name: "Mic/Aux" -> "mic_aux". */
export function slug(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "") || "input";
}

interface State {
  client: ObsClient | null;
  /** Why we are not connected, for the person reading the plugin's status. */
  reason: string;
  scenes: string[];
  inputs: Map<string, string>; // slug -> OBS name, audio inputs only
  retry: ReturnType<typeof setTimeout> | null;
  attempt: number;
  stopping: boolean;
}

const state: State = {
  client: null,
  reason: "not started",
  scenes: [],
  inputs: new Map(),
  retry: null,
  attempt: 0,
  stopping: false,
};

/** Injected by the tests; the real WebSocket otherwise. */
let connectOverride: Connect | undefined;
export function useConnection(connect: Connect | undefined): void {
  connectOverride = connect;
}

function client(): ObsClient {
  if (!state.client?.connected) {
    throw new Error(`OBS is not connected (${state.reason})`);
  }
  return state.client;
}

/** Run a request, turning OBS's refusals into sentences. */
async function request<T = Record<string, unknown>>(type: string, data?: Record<string, unknown>): Promise<T> {
  try {
    return await client().request<T>(type, data);
  } catch (error) {
    // The message alone: "Error: …" in front of it reads as noise to a person.
    throw new Error(error instanceof Error ? error.message : String(error));
  }
}

function timecode(raw: unknown): string {
  // OBS sends "HH:MM:SS.mmm"; a deck has no room for milliseconds.
  return typeof raw === "string" ? raw.split(".")[0] ?? raw : "00:00:00";
}

/** Read everything once after connecting; events keep it current after. */
async function refreshAll(ctx: Context): Promise<void> {
  const scenes = await request<{ currentProgramSceneName: string; scenes: { sceneName: string }[] }>("GetSceneList");
  state.scenes = scenes.scenes.map((scene) => scene.sceneName).reverse();
  const record = await request<{ outputActive: boolean; outputPaused: boolean; outputTimecode: string }>("GetRecordStatus");
  const stream = await request<{ outputActive: boolean; outputTimecode: string }>("GetStreamStatus");
  const studio = await request<{ studioModeEnabled: boolean }>("GetStudioModeEnabled");
  ctx.vars.set({
    "obs.connected": true,
    "obs.scene": scenes.currentProgramSceneName,
    "obs.recording": record.outputActive,
    "obs.record_paused": record.outputPaused,
    "obs.record_time": timecode(record.outputTimecode),
    "obs.streaming": stream.outputActive,
    "obs.stream_time": timecode(stream.outputTimecode),
    "obs.studio_mode": studio.studioModeEnabled,
  });
  await refreshInputs(ctx);
}

/**
 * The volume variable of an input, by OBS's own name for it — not the slug —
 * because a slider's feedback template is filled with the argument as chosen,
 * which is OBS's name: `obs.input.{{input}}.volume` → `obs.input.Mic/Aux.volume`.
 */
export const volumeVariable = (name: string) => `obs.input.${name}.volume`;

/** OBS's linear multiplier as the percentage a slider shows. */
const percentOf = (multiplier: unknown) => Math.round(Math.max(0, Math.min(1, Number(multiplier) || 0)) * 100);

/**
 * One `obs.input.<slug>.muted` and one `obs.input.<name>.volume` per audio
 * input, declared as OBS has them.
 */
async function refreshInputs(ctx: Context): Promise<void> {
  const { inputs } = await request<{ inputs: { inputName: string }[] }>("GetInputList");
  const audio = new Map<string, string>();
  const values: Record<string, boolean | number> = {};
  for (const input of inputs) {
    try {
      const answer = await request<{ inputMuted: boolean }>("GetInputMute", { inputName: input.inputName });
      audio.set(slug(input.inputName), input.inputName);
      values[`obs.input.${slug(input.inputName)}.muted`] = answer.inputMuted;
    } catch {
      // Not an audio input; OBS refuses GetInputMute for those.
      continue;
    }
    try {
      const volume = await request<{ inputVolumeMul: number }>("GetInputVolume", { inputName: input.inputName });
      values[volumeVariable(input.inputName)] = percentOf(volume.inputVolumeMul);
    } catch {
      // An older OBS: the slider still works, it just will not follow.
    }
  }
  state.inputs = audio;
  const declared: Record<string, VariableSpec> = {};
  for (const [key, name] of audio) {
    declared[`obs.input.${key}.muted`] = { kind: "bool", description: `Whether "${name}" is muted` };
    declared[volumeVariable(name)] = { kind: "number", unit: "%", description: `The volume of "${name}"` };
  }
  ctx.vars.declare(declared, { replace: true });
  ctx.vars.set(values);
}

function onEvent(ctx: Context, type: string, data: Record<string, unknown>): void {
  switch (type) {
    case "CurrentProgramSceneChanged": {
      const scene = String(data.sceneName);
      ctx.vars.set({ "obs.scene": scene });
      ctx.emit("scene_changed", { scene });
      break;
    }
    case "SceneListChanged":
      state.scenes = ((data.scenes as { sceneName: string }[]) ?? []).map((scene) => scene.sceneName).reverse();
      break;
    case "RecordStateChanged": {
      const active = Boolean(data.outputActive);
      const outputState = String(data.outputState);
      const paused = outputState === "OBS_WEBSOCKET_OUTPUT_PAUSED";
      ctx.vars.set({ "obs.recording": active, "obs.record_paused": paused });
      if (!active) ctx.vars.set({ "obs.record_time": "00:00:00" });
      const word = outputStateWord(outputState);
      if (word) ctx.emit("record_state_changed", { state: word });
      break;
    }
    case "StreamStateChanged": {
      const active = Boolean(data.outputActive);
      ctx.vars.set({ "obs.streaming": active });
      if (!active) ctx.vars.set({ "obs.stream_time": "00:00:00" });
      const word = outputStateWord(String(data.outputState));
      if (word) ctx.emit("stream_state_changed", { state: word });
      break;
    }
    case "StudioModeStateChanged":
      ctx.vars.set({ "obs.studio_mode": Boolean(data.studioModeEnabled) });
      break;
    case "InputMuteStateChanged": {
      const name = String(data.inputName);
      const muted = Boolean(data.inputMuted);
      ctx.vars.set({ [`obs.input.${slug(name)}.muted`]: muted });
      ctx.emit("input_mute_changed", { input: name, muted: String(muted) });
      break;
    }
    case "InputVolumeChanged": {
      const name = String(data.inputName);
      if ([...state.inputs.values()].includes(name)) {
        ctx.vars.set({ [volumeVariable(name)]: percentOf(data.inputVolumeMul) });
      }
      break;
    }
    case "InputCreated":
    case "InputRemoved":
    case "InputNameChanged":
      void refreshInputs(ctx).catch(() => {});
      break;
    case "ConnectionClosed":
      disconnected(ctx, "OBS closed the connection");
      break;
  }
}

/** Only the settled states: OBS also reports "starting" and "stopping". */
function outputStateWord(outputState: string): string | undefined {
  switch (outputState) {
    case "OBS_WEBSOCKET_OUTPUT_STARTED":
      return "started";
    case "OBS_WEBSOCKET_OUTPUT_STOPPED":
      return "stopped";
    case "OBS_WEBSOCKET_OUTPUT_PAUSED":
      return "paused";
    case "OBS_WEBSOCKET_OUTPUT_RESUMED":
      return "resumed";
    default:
      return undefined;
  }
}

function disconnected(ctx: Context, reason: string): void {
  // `state.client` is set only once a connection succeeded, so its presence
  // — not `connected`, which the socket has already cleared — says "we were".
  const was = state.client !== null;
  state.client?.close();
  state.client = null;
  state.reason = reason;
  ctx.vars.set({ "obs.connected": false });
  if (was) ctx.emit("disconnected", { reason });
  schedule(ctx);
}

/** Try again in 1, 2, 4 … 30 seconds. OBS restarting is normal, not an error. */
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
  const port = ctx.settings.port || "4455";
  const obs = new ObsClient({ url: `ws://${host}:${port}`, password: ctx.settings.password, connect: connectOverride });
  try {
    await obs.open();
  } catch (error) {
    state.reason = error instanceof Error ? error.message : String(error);
    ctx.vars.set({ "obs.connected": false });
    ctx.log.warn(state.reason);
    schedule(ctx);
    return;
  }
  state.client = obs;
  state.attempt = 0;
  state.reason = "connected";
  obs.on((type, data) => onEvent(ctx, type, data));
  try {
    await refreshAll(ctx);
  } catch (error) {
    ctx.log.error(`connected, but could not read OBS's state: ${String(error)}`);
  }
  ctx.emit("connected", {});
  ctx.log.info(`connected to OBS at ${host}:${port}`);
}

export function disconnect(): void {
  state.stopping = true;
  if (state.retry) clearTimeout(state.retry);
  state.retry = null;
  state.client?.close();
  state.client = null;
}

/** For the tests: forget everything between cases. */
export function reset(): void {
  disconnect();
  state.stopping = false;
  state.attempt = 0;
  state.reason = "not started";
  state.scenes = [];
  state.inputs = new Map();
}

function input(name: string): string {
  // Accept either OBS's own name or the slug the variables use.
  return state.inputs.get(slug(name)) ?? name;
}

async function sceneItem(scene: string, source: string): Promise<number> {
  const { sceneItemId } = await request<{ sceneItemId: number }>("GetSceneItemId", { sceneName: scene, sourceName: source });
  return sceneItemId;
}

/*
 * What the editor offers while someone sets up a button. Asked fresh each
 * time, not read from what was cached at connect, so a scene added a minute
 * ago is there. When OBS is not connected these throw, and the editor shows
 * why and lets the person type a name instead.
 */

/** The scenes, top to bottom as OBS's Scenes panel lists them. */
async function sceneChoices(): Promise<string[]> {
  const { scenes } = await request<{ scenes: { sceneName: string }[] }>("GetSceneList");
  // OBS lists them bottom-up; its panel shows them the other way round.
  return scenes.map((scene) => scene.sceneName).reverse();
}

/** The inputs that have audio — the only ones that can be muted. */
async function inputChoices(): Promise<string[]> {
  client(); // says "not connected" rather than offering a stale list
  return [...state.inputs.values()].sort((a, b) => a.localeCompare(b));
}

/**
 * The sources in `scene`, top to bottom as OBS's Sources panel lists them —
 * or `null` before a scene is chosen: there is nothing to list *yet*, which
 * is not the same as a scene with no sources.
 */
async function sourceChoices(scene: string | undefined): Promise<string[] | null> {
  if (!scene?.trim()) return null;
  const { sceneItems } = await request<{ sceneItems: { sourceName: string }[] }>("GetSceneItemList", {
    sceneName: scene,
  });
  return sceneItems.map((item) => item.sourceName).reverse();
}

const unknownScene = (scene: string): string | undefined =>
  state.client?.connected && state.scenes.length > 0 && !state.scenes.includes(scene)
    ? `OBS has no scene called '${scene}' (it has: ${state.scenes.join(", ")})`
    : undefined;

export default definePlugin({
  id: ID,
  name: "OBS Studio",
  version: "0.3.0",
  description: "Switch scenes, record, stream, and mute inputs in OBS Studio — and show its state on the deck.",
  author: "flexdeck",
  permissions: ["network"],

  settings: {
    host: { kind: "text", description: "The computer OBS runs on", default: "127.0.0.1" },
    port: { kind: "number", description: "OBS's WebSocket port (Tools → WebSocket Server Settings)", default: "4455" },
    password: { kind: "text", description: "The WebSocket server password, if it has one", secret: true },
  },

  variables: {
    "obs.connected": { kind: "bool", description: "Whether OBS is connected" },
    "obs.scene": { kind: "text", description: "The live (program) scene" },
    "obs.recording": { kind: "bool", description: "Whether OBS is recording" },
    "obs.record_paused": { kind: "bool", description: "Whether the recording is paused" },
    "obs.record_time": { kind: "text", description: "How long it has been recording (HH:MM:SS)" },
    "obs.streaming": { kind: "bool", description: "Whether OBS is streaming" },
    "obs.stream_time": { kind: "text", description: "How long it has been live (HH:MM:SS)" },
    "obs.studio_mode": { kind: "bool", description: "Whether studio mode is on" },
  },

  events: {
    connected: { description: "OBS connected" },
    disconnected: { description: "OBS went away", fields: ["reason"] },
    scene_changed: { description: "The live scene changed", fields: ["scene"] },
    record_state_changed: { description: "Recording started, stopped, paused or resumed", fields: ["state"] },
    stream_state_changed: { description: "Streaming started or stopped", fields: ["state"] },
    input_mute_changed: { description: "An input was muted or unmuted", fields: ["input", "muted"] },
  },

  actions: {
    set_scene: {
      description: "Switch the live scene",
      arguments: {
        scene: {
          kind: "text",
          description: "The scene's name in OBS",
          example: "Scene",
          suggest: () => sceneChoices(),
        },
      },
      check: ({ scene }) => unknownScene(scene),
      run: async ({ scene }) => {
        await request("SetCurrentProgramScene", { sceneName: scene });
      },
    },
    set_preview_scene: {
      description: "Put a scene in preview (studio mode)",
      arguments: {
        scene: {
          kind: "text",
          description: "The scene's name in OBS",
          example: "Scene",
          suggest: () => sceneChoices(),
        },
      },
      check: ({ scene }) => unknownScene(scene),
      run: async ({ scene }) => {
        await request("SetCurrentPreviewScene", { sceneName: scene });
      },
    },
    transition: {
      description: "Send the preview scene live (studio mode)",
      run: async () => {
        await request("TriggerStudioModeTransition");
      },
    },
    toggle_studio_mode: {
      description: "Turn studio mode on or off",
      run: async () => {
        const { studioModeEnabled } = await request<{ studioModeEnabled: boolean }>("GetStudioModeEnabled");
        await request("SetStudioModeEnabled", { studioModeEnabled: !studioModeEnabled });
      },
    },
    start_recording: { description: "Start recording", run: async () => void (await request("StartRecord")) },
    stop_recording: { description: "Stop recording", run: async () => void (await request("StopRecord")) },
    toggle_recording: { description: "Start or stop recording", run: async () => void (await request("ToggleRecord")) },
    toggle_recording_pause: {
      description: "Pause or resume the recording",
      run: async () => void (await request("ToggleRecordPause")),
    },
    start_streaming: { description: "Go live", run: async () => void (await request("StartStream")) },
    stop_streaming: { description: "End the stream", run: async () => void (await request("StopStream")) },
    toggle_streaming: { description: "Go live, or end the stream", run: async () => void (await request("ToggleStream")) },
    save_replay: {
      description: "Save the replay buffer",
      run: async () => void (await request("SaveReplayBuffer")),
    },
    toggle_virtual_camera: {
      description: "Start or stop the virtual camera",
      run: async () => void (await request("ToggleVirtualCam")),
    },
    toggle_mute: {
      description: "Toggle an input's mute",
      arguments: { input: { kind: "text", description: "The input's name in OBS", example: "Mic/Aux", suggest: () => inputChoices() } },
      run: async ({ input: name }) => void (await request("ToggleInputMute", { inputName: input(name) })),
    },
    set_mute: {
      description: "Set an input to muted or unmuted",
      arguments: {
        input: { kind: "text", description: "The input's name in OBS", example: "Mic/Aux", suggest: () => inputChoices() },
        muted: { kind: "choice", description: "Muted or not", example: "true", choices: ["true", "false"] },
      },
      run: async ({ input: name, muted }) =>
        void (await request("SetInputMute", { inputName: input(name), inputMuted: muted === "true" })),
    },
    set_volume: {
      description: "Set an input's volume",
      arguments: {
        input: { kind: "text", description: "The input's name in OBS", example: "Mic/Aux", suggest: () => inputChoices() },
        percent: { kind: "number", description: "0 to 100", example: "80" },
      },
      // The muted variable is keyed by a slug of the name, which a template
      // cannot make, so the tile mutes but does not show it.
      slider: { argument: "percent", feedback: "obs.input.{{input}}.volume", mute: { action: "toggle_mute" } },
      check: ({ percent }) => {
        const value = Number(percent);
        return Number.isFinite(value) && value >= 0 && value <= 100 ? undefined : "the volume must be 0 to 100";
      },
      run: async ({ input: name, percent }) =>
        void (await request("SetInputVolume", { inputName: input(name), inputVolumeMul: Number(percent) / 100 })),
    },
    toggle_source: {
      description: "Show or hide a source in a scene",
      arguments: {
        scene: { kind: "text", description: "The scene", example: "Scene", suggest: () => sceneChoices() },
        source: {
          kind: "text",
          description: "The source in that scene",
          example: "Webcam",
          suggest: ({ scene }) => sourceChoices(scene),
        },
      },
      check: ({ scene }) => unknownScene(scene),
      run: async ({ scene, source }) => {
        const id = await sceneItem(scene, source);
        const { sceneItemEnabled } = await request<{ sceneItemEnabled: boolean }>("GetSceneItemEnabled", {
          sceneName: scene,
          sceneItemId: id,
        });
        await request("SetSceneItemEnabled", { sceneName: scene, sceneItemId: id, sceneItemEnabled: !sceneItemEnabled });
      },
    },
  },

  // Only the timecodes are polled, and only while the deck shows them.
  sample: async (wanted) => {
    if (!state.client?.connected) return { "obs.connected": false };
    const values: Record<string, string | boolean> = {};
    if (wanted.includes("obs.record_time")) {
      const record = await request<{ outputTimecode: string }>("GetRecordStatus");
      values["obs.record_time"] = timecode(record.outputTimecode);
    }
    if (wanted.includes("obs.stream_time")) {
      const stream = await request<{ outputTimecode: string }>("GetStreamStatus");
      values["obs.stream_time"] = timecode(stream.outputTimecode);
    }
    return values;
  },

  start: (ctx) => {
    state.stopping = false;
    ctx.vars.set({ "obs.connected": false });
    void connect(ctx);
  },

  // New host, port or password: drop the connection and use the new ones.
  settingsChanged: (ctx) => {
    reset();
    void connect(ctx);
  },

  stop: () => disconnect(),
});
