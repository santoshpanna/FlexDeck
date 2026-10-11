/**
 * An in-memory OBS: the server side of obs-websocket 5, enough of it to drive
 * the plugin. Changes made through requests are announced as events, the way
 * OBS does, so a test sees the same sequence the real thing would produce.
 */

import { randomBytes } from "node:crypto";

import { authResponse, type Connect, type SocketLike } from "../src/obs-client.ts";

export interface FakeObsOptions {
  password?: string;
  scenes?: string[];
  inputs?: Record<string, boolean>; // name -> muted
  /** Refuse to connect at all, as if OBS were not running. */
  offline?: boolean;
}

export class FakeObs {
  scene: string;
  scenes: string[];
  inputs: Map<string, { muted: boolean; volume: number }>;
  recording = false;
  paused = false;
  streaming = false;
  studioMode = false;
  sources = new Map<string, boolean>([["Scene/Webcam", true]]);
  requests: string[] = [];
  sockets: FakeSocket[] = [];
  offline: boolean;

  readonly options: FakeObsOptions;

  constructor(options: FakeObsOptions = {}) {
    this.options = options;
    this.scenes = options.scenes ?? ["Scene", "Live", "BRB"];
    this.scene = this.scenes[0] ?? "Scene";
    this.inputs = new Map(Object.entries(options.inputs ?? { "Mic/Aux": false, "Desktop Audio": true }).map(([name, muted]) => [name, { muted, volume: 1 }]));
    this.offline = options.offline ?? false;
  }

  readonly connect: Connect = () => {
    const socket = new FakeSocket(this);
    this.sockets.push(socket);
    return socket;
  };

  /** Push an event to every identified client, as OBS would. */
  event(eventType: string, eventData: Record<string, unknown>): void {
    for (const socket of this.sockets) socket.deliver({ op: 5, d: { eventType, eventIntent: 0, eventData } });
  }

  /** Drop every connection, as if OBS quit. */
  quit(): void {
    this.offline = true;
    for (const socket of this.sockets) socket.drop(1001, "OBS is closing");
    this.sockets = [];
  }

  handle(type: string, data: Record<string, unknown>): Record<string, unknown> {
    this.requests.push(type);
    const output = (active: boolean, state: string) => ({ outputActive: active, outputState: `OBS_WEBSOCKET_OUTPUT_${state}` });
    switch (type) {
      case "GetSceneList":
        return { currentProgramSceneName: this.scene, scenes: [...this.scenes].reverse().map((sceneName) => ({ sceneName })) };
      case "SetCurrentProgramScene": {
        const scene = String(data.sceneName);
        if (!this.scenes.includes(scene)) throw new Error(`No source was found by the name of \`${scene}\`.`);
        this.scene = scene;
        this.event("CurrentProgramSceneChanged", { sceneName: scene });
        return {};
      }
      case "GetRecordStatus":
        return { outputActive: this.recording, outputPaused: this.paused, outputTimecode: this.recording ? "00:01:05.250" : "00:00:00.000" };
      case "GetStreamStatus":
        return { outputActive: this.streaming, outputTimecode: this.streaming ? "01:00:00.000" : "00:00:00.000" };
      case "GetStudioModeEnabled":
        return { studioModeEnabled: this.studioMode };
      case "SetStudioModeEnabled":
        this.studioMode = Boolean(data.studioModeEnabled);
        this.event("StudioModeStateChanged", { studioModeEnabled: this.studioMode });
        return {};
      case "ToggleRecord":
      case "StartRecord":
      case "StopRecord": {
        this.recording = type === "ToggleRecord" ? !this.recording : type === "StartRecord";
        this.event("RecordStateChanged", output(this.recording, this.recording ? "STARTED" : "STOPPED"));
        return {};
      }
      case "ToggleStream": {
        this.streaming = !this.streaming;
        this.event("StreamStateChanged", output(this.streaming, this.streaming ? "STARTED" : "STOPPED"));
        return {};
      }
      case "GetInputList":
        return { inputs: [...this.inputs.keys(), "Browser Source"].map((inputName) => ({ inputName })) };
      case "GetInputMute": {
        const input = this.inputs.get(String(data.inputName));
        if (!input) throw new Error("The specified input does not support audio.");
        return { inputMuted: input.muted };
      }
      case "ToggleInputMute":
      case "SetInputMute": {
        const name = String(data.inputName);
        const input = this.inputs.get(name);
        if (!input) throw new Error(`No source was found by the name of \`${name}\`.`);
        input.muted = type === "ToggleInputMute" ? !input.muted : Boolean(data.inputMuted);
        this.event("InputMuteStateChanged", { inputName: name, inputMuted: input.muted });
        return {};
      }
      case "GetInputVolume": {
        const input = this.inputs.get(String(data.inputName));
        if (!input) throw new Error("No such input.");
        return { inputVolumeMul: input.volume, inputVolumeDb: 20 * Math.log10(input.volume || 1e-9) };
      }
      case "SetInputVolume": {
        const name = String(data.inputName);
        const input = this.inputs.get(name);
        if (!input) throw new Error("No such input.");
        input.volume = Number(data.inputVolumeMul);
        this.event("InputVolumeChanged", { inputName: name, inputVolumeMul: input.volume });
        return {};
      }
      case "GetSceneItemList": {
        const scene = String(data.sceneName);
        if (!this.scenes.includes(scene)) throw new Error(`No source was found by the name of \`${scene}\`.`);
        // Bottom-up, as OBS answers.
        const items = [...this.sources.keys()]
          .filter((key) => key.startsWith(`${scene}/`))
          .map((key, index) => ({ sourceName: key.slice(scene.length + 1), sceneItemId: index + 1 }));
        return { sceneItems: items.reverse() };
      }
      case "GetSceneItemId":
        if (!this.sources.has(`${String(data.sceneName)}/${String(data.sourceName)}`)) throw new Error("No scene items were found.");
        return { sceneItemId: 7 };
      case "GetSceneItemEnabled":
        return { sceneItemEnabled: this.sources.get("Scene/Webcam") };
      case "SetSceneItemEnabled":
        this.sources.set("Scene/Webcam", Boolean(data.sceneItemEnabled));
        return {};
      default:
        throw new Error(`the fake OBS does not know ${type}`);
    }
  }
}

class FakeSocket implements SocketLike {
  onopen: SocketLike["onopen"] = null;
  onmessage: SocketLike["onmessage"] = null;
  onclose: SocketLike["onclose"] = null;
  onerror: SocketLike["onerror"] = null;
  private identified = false;
  private closed = false;
  private readonly salt = randomBytes(8).toString("base64");
  private readonly challenge = randomBytes(8).toString("base64");

  private readonly obs: FakeObs;

  constructor(obs: FakeObs) {
    this.obs = obs;
    setImmediate(() => {
      if (obs.offline) {
        this.onerror?.({});
        this.drop(1006, "");
        return;
      }
      const authentication = obs.options.password ? { salt: this.salt, challenge: this.challenge } : undefined;
      this.reply({ op: 0, d: { obsWebSocketVersion: "5.5.0", rpcVersion: 1, authentication } });
    });
  }

  send(raw: string): void {
    const message = JSON.parse(raw) as { op: number; d: Record<string, unknown> };
    if (message.op === 1) {
      const password = this.obs.options.password;
      if (password && message.d.authentication !== authResponse(password, this.salt, this.challenge)) {
        this.drop(4009, "Authentication failed.");
        return;
      }
      this.identified = true;
      this.reply({ op: 2, d: { negotiatedRpcVersion: 1 } });
      return;
    }
    if (message.op === 6) {
      const { requestType, requestId, requestData } = message.d as { requestType: string; requestId: string; requestData?: Record<string, unknown> };
      let responseData: Record<string, unknown> = {};
      let requestStatus: Record<string, unknown> = { result: true, code: 100 };
      try {
        responseData = this.obs.handle(requestType, requestData ?? {});
      } catch (error) {
        requestStatus = { result: false, code: 600, comment: (error as Error).message };
      }
      this.reply({ op: 7, d: { requestType, requestId, requestStatus, responseData } });
    }
  }

  deliver(message: unknown): void {
    if (this.identified) this.reply(message);
  }

  close(): void {
    this.drop(1000, "");
  }

  drop(code: number, reason: string): void {
    if (this.closed) return;
    this.closed = true;
    this.identified = false;
    setImmediate(() => this.onclose?.({ code, reason }));
  }

  private reply(message: unknown): void {
    if (this.closed) return;
    setImmediate(() => this.onmessage?.({ data: JSON.stringify(message) }));
  }
}
