/**
 * The flexdeck plugin SDK.
 *
 * A plugin is one call to {@link definePlugin}: what it is, what it can do, the
 * values it owns, and the events it can announce. The SDK speaks the protocol;
 * the plugin only describes itself and does its work.
 *
 * ```ts
 * import { definePlugin } from "@flexdeck/plugin-sdk";
 *
 * export default definePlugin({
 *   id: "hello",
 *   name: "Hello",
 *   version: "1.0.0",
 *   actions: {
 *     greet: {
 *       description: "Say hello",
 *       arguments: { name: { kind: "text", example: "world" } },
 *       run: ({ name }, ctx) => ctx.notify("Hello", name),
 *     },
 *   },
 * });
 * ```
 *
 * When the agent starts the plugin it sets `FLEXDECK_PLUGIN_ID`, and the
 * definition starts serving on stdin/stdout by itself. Imported anywhere else —
 * a test, say — it does nothing until asked, so a definition can be inspected
 * and its actions called directly.
 */

import { createHash } from "node:crypto";
import { createInterface } from "node:readline";

import {
  ABI_VERSION,
  type ActionDto,
  type ArgumentDto,
  Capability,
  type FeedDto,
  MAX_SERIES_POINTS,
  type PresetDto,
  type EventDto,
  IMAGE_MIMES,
  type ImageMime,
  type InvokeResponse,
  isValidId,
  MAX_IMAGE_BYTES,
  type Kind,
  type ManifestDto,
  type ChoiceDto,
  type ChoicesResponse,
  type Message,
  Method,
  Notify,
  type Permission,
  type SettingDto,
  type SliderDto,
  SUPPORTED_ABIS,
  type ValueDto,
  type VariableDto,
} from "./protocol.ts";

export * from "./protocol.ts";

export type Value = boolean | number | string;

export interface ArgumentSpec {
  kind: Kind;
  description?: string;
  /** A value that works. The editor prefills it, and tests run it. */
  example: string;
  choices?: string[];
  optional?: boolean;
  /**
   * The values this argument can take *right now*, for the editor to offer —
   * OBS's scene names, say. `args` is what is filled in so far, so one
   * argument can depend on another (the sources in the chosen scene). Throw
   * when they cannot be had ("OBS is not running"); the editor shows why and
   * lets the person type instead. Return `null` when there is nothing to offer
   * *yet* — no scene chosen, so no sources — and the field is typed until there is.
   */
  suggest?: (
    args: Record<string, string>,
    ctx: Context,
  ) => Suggestion[] | null | Promise<Suggestion[] | null>;
}

/** A suggested value: the value itself, or the value with a label to show. */
export type Suggestion = string | { value: string; label?: string };

/**
 * Lets a slider drive an action (ABI 3). The slider's value is written into
 * `argument`; `feedback` names the variable that reports the live value, so
 * the thumb follows changes made elsewhere. It may use `{{argument}}`
 * placeholders filled from the action's other arguments:
 * `"discord.user.{{user}}.volume"`.
 */
export interface SliderSpec<Args extends string = string> {
  argument: Args;
  min?: number;
  max?: number;
  step?: number;
  feedback?: string;
  /**
   * How the same channel is muted, so the slider tile can mute from its
   * header: an action of this plugin that toggles it (run with this action's
   * other arguments), and the variable saying whether it is muted.
   */
  mute?: { action: string; feedback?: string };
}

/** What `run` may return: the look a widget driven by this action should show. */
export interface RunResult {
  state?: string;
}

export interface ActionSpec<Args extends string = string> {
  description: string;
  arguments?: Record<Args, ArgumentSpec>;
  /** A slider can drive this action. */
  slider?: SliderSpec<Args>;
  /**
   * Refuse a value before anything runs: return a reason, or nothing if it is
   * fine. Called when an automation is saved and before every press.
   */
  check?: (args: Record<Args, string>, ctx: Context) => string | undefined | void | Promise<string | undefined | void>;
  /**
   * Do the thing. Throw to fail; the message reaches the person. Return
   * `{ state }` to tell a widget which of its looks applies now.
   */
  run: (args: Record<Args, string>, ctx: Context) => void | RunResult | Promise<void | RunResult>;
}

export interface SettingSpec {
  kind: Kind;
  description?: string;
  default?: string;
  choices?: string[];
  /** A password or token: stored, given to the plugin, never shown again. */
  secret?: boolean;
}

export interface VariableSpec {
  kind: Kind;
  description?: string;
  unit?: string;
  choices?: string[];
  /** Owned by the plugin; automations cannot set it. Defaults to `true`. */
  readOnly?: boolean;
}

export interface EventSpec {
  description?: string;
  /** The payload keys, so the editor can offer a filter. */
  fields?: string[];
}

/** A ready-made key for the editor's gallery (ABI 4). */
export interface PresetSpec {
  id: string;
  name: string;
  category?: string;
  description?: string;
  /** Its size in cells. */
  w?: number;
  h?: number;
  /** A widget as the configuration holds it, without id or place: its face, flows, states. */
  widget: Record<string, unknown>;
}

/**
 * Values a key gets for itself, with its own arguments (ABI 4): a weather key
 * per city. The key's templates read them as `{{this.<field>}}`; publish them
 * with `ctx.feed(instance).set({...})`.
 */
export interface FeedSpec<Args extends string = string> {
  description?: string;
  arguments?: Record<Args, ArgumentSpec>;
  /** What it publishes per key, by short name. */
  fields: Record<string, { kind: Kind; description?: string }>;
  /** A key started using it: start publishing for `instance`. */
  watch: (instance: string, args: Record<Args, string>, ctx: Context) => void | Promise<void>;
  /** That key is gone: stop. */
  unwatch?: (instance: string, ctx: Context) => void | Promise<void>;
}

export interface PluginDefinition {
  id: string;
  name: string;
  version: string;
  description?: string;
  author?: string;
  permissions?: Permission[];
  settings?: Record<string, SettingSpec>;
  /** Variables known up front, by full name (`<id>.<something>`). */
  variables?: Record<string, VariableSpec>;
  events?: Record<string, EventSpec>;
  actions: Record<string, ActionSpec>;
  /** Ready-made keys for the gallery. ABI 4. */
  presets?: PresetSpec[];
  /** Values per key. ABI 4. */
  feeds?: Record<string, FeedSpec>;
  /** After `initialize`: connect, subscribe, start timers. */
  start?: (ctx: Context) => void | Promise<void>;
  /** The user saved new settings. `ctx.settings` already holds them. */
  settingsChanged?: (ctx: Context) => void | Promise<void>;
  /**
   * Refresh `wanted` — only what someone is watching. Return values to
   * publish, or publish them with `ctx.vars.set` and return nothing.
   */
  sample?: (wanted: string[], ctx: Context) => void | Record<string, Value> | Promise<void | Record<string, Value>>;
  /** Before exit: close connections. */
  stop?: (ctx: Context) => void | Promise<void>;
}

export interface Logger {
  info(message: string): void;
  warn(message: string): void;
  error(message: string): void;
}

/** Everything a plugin can do to the world, through the host. */
export interface Context {
  readonly id: string;
  /** This plugin's settings, defaults filled in. */
  readonly settings: Readonly<Record<string, string>>;
  /** Where the plugin may keep files. */
  readonly dataDir: string | null;
  readonly vars: {
    /** Publish values for variables this plugin owns. */
    set(values: Record<string, Value>): void;
    /** Declare variables at run time, e.g. one per scene. */
    declare(variables: Record<string, VariableSpec>, options?: { replace?: boolean }): void;
    /** The last value this plugin published. */
    get(name: string): Value | undefined;
  };
  /** Announce an event the definition declares. */
  emit(event: string, payload?: Record<string, string>): void;
  /** Show a notification. Needs the `notifications` permission. */
  notify(title: string, body?: string): void;
  /**
   * Keep values for settings this plugin declares — a refresh token it was
   * just issued, say. They survive a restart and come back in `ctx.settings`.
   * Keys the definition does not declare are refused. ABI 3.
   */
  storeSettings(values: Record<string, string>): void;
  /** Open an http(s) address in the person's browser. Needs `network`. ABI 3. */
  openUrl(url: string): void;
  readonly images: {
    /**
     * Hand the agent an image and get back the value a variable can hold for
     * a widget to show it: `img:<sha256>`. At most 256 KiB. ABI 3.
     */
    put(bytes: Uint8Array, mime: ImageMime): string;
  };
  readonly log: Logger;
  readonly series: {
    /**
     * A variable's whole recent history, oldest first and evenly spaced, for a
     * graph: a day of prices, a forecast's hours. At most 600 points. ABI 4.
     */
    set(name: string, points: number[]): void;
  };
  /**
   * Publish values for one key's run of a feed: `ctx.feed(instance).set({temp: 21})`
   * becomes `<id>.@<instance>.temp`, read by that key as `{{this.temp}}`. ABI 4.
   */
  feed(instance: string): { set(values: Record<string, Value>): void };
}

/** A plugin, defined. */
export interface Plugin {
  readonly definition: PluginDefinition;
  /** What `initialize` answers with, at `abi` (the newest by default). */
  manifest(abi?: number): ManifestDto;
  variables(): VariableDto[];
  /** Serve the protocol on this process's stdin/stdout. */
  serve(): void;
  /** Handle one request, as the host would send it. For tests. */
  handle(method: string, params: unknown): Promise<unknown>;
  /** What the plugin has sent to the host, unprompted. For tests. */
  readonly sent: Message[];
  /** Set settings as `initialize` would, *without* calling `start`. For tests. */
  configure(settings: Record<string, string>, dataDir?: string | null): void;
  /** Speak `abi` from now on, as `initialize` would. For tests. */
  useAbi(abi: number): void;
  /** The context actions receive. For tests. */
  readonly context: Context;
}

class DefinitionError extends Error {}

function check(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new DefinitionError(`flexdeck plugin: ${message}`);
  }
}

function validate(definition: PluginDefinition): void {
  check(isValidId(definition.id), `'${definition.id}' is not a usable id (lowercase letters, digits, - and _)`);
  const prefix = `${definition.id}.`;
  for (const name of Object.keys(definition.variables ?? {})) {
    check(name.startsWith(prefix) && name.length > prefix.length, `variable '${name}' must start with '${prefix}'`);
  }
  for (const [name, action] of Object.entries(definition.actions)) {
    for (const [argument, spec] of Object.entries(action.arguments ?? {}) as [string, ArgumentSpec][]) {
      if (spec.kind === "choice") {
        check(spec.choices?.includes(spec.example), `${name}.${argument}: the example must be one of its choices`);
      }
    }
    if (action.slider) {
      const { argument, min = 0, max = 100, step = 1 } = action.slider;
      check(argument in (action.arguments ?? {}), `${name}.slider: '${argument}' is not one of its arguments`);
      check(min < max && step > 0, `${name}.slider: needs min < max and step > 0`);
    }
  }
}

function argumentDtos(args: Record<string, ArgumentSpec> | undefined): ArgumentDto[] {
  return (Object.entries(args ?? {}) as [string, ArgumentSpec][]).map(([name, argument]) => ({
    name,
    description: argument.description ?? "",
    example: argument.example,
    kind: argument.kind,
    choices: argument.choices ?? [],
    optional: argument.optional ?? false,
  }));
}

function manifestOf(definition: PluginDefinition, abi: number = ABI_VERSION): ManifestDto {
  const actions: ActionDto[] = Object.entries(definition.actions).map(([action, spec]) => {
    const dto: ActionDto = {
      action,
      description: spec.description,
      arguments: (Object.entries(spec.arguments ?? {}) as [string, ArgumentSpec][]).map(([name, argument]) => ({
        name,
        description: argument.description ?? "",
        example: argument.example,
        kind: argument.kind,
        choices: argument.choices ?? [],
        optional: argument.optional ?? false,
      })),
    };
    // ABI 2 refuses fields it does not know, so they are left out under it.
    if (abi >= 3 && spec.slider) {
      const { argument, min = 0, max = 100, step = 1, feedback, mute } = spec.slider;
      const slider: SliderDto = { argument, min, max, step };
      if (feedback !== undefined) slider.feedback = feedback;
      if (mute !== undefined) slider.mute = mute.feedback === undefined ? { action: mute.action } : { ...mute };
      dto.slider = slider;
    }
    return dto;
  });
  const settings: SettingDto[] = Object.entries(definition.settings ?? {}).map(([key, setting]) => ({
    key,
    description: setting.description ?? "",
    kind: setting.kind,
    choices: setting.choices ?? [],
    defaultValue: setting.default ?? null,
    secret: setting.secret ?? false,
  }));
  const events: EventDto[] = Object.entries(definition.events ?? {}).map(([name, event]) => ({
    name,
    description: event.description ?? "",
    fields: event.fields ?? [],
  }));
  const manifest: ManifestDto = {
    abi,
    id: definition.id,
    name: definition.name,
    version: definition.version,
    description: definition.description ?? "",
    author: definition.author ?? "",
    actions,
    settings,
    events,
    permissions: definition.permissions ?? [],
  };
  // ABI 4's additions; an older agent refuses fields it does not know.
  if (abi >= 4) {
    const uses: Capability[] = [Capability.series];
    if (definition.presets?.length) {
      uses.push(Capability.presets);
      manifest.presets = definition.presets.map(
        (preset): PresetDto => ({
          id: preset.id,
          name: preset.name,
          category: preset.category ?? "",
          description: preset.description ?? "",
          w: preset.w ?? 1,
          h: preset.h ?? 1,
          widget: preset.widget,
        }),
      );
    }
    const feeds = Object.entries(definition.feeds ?? {});
    if (feeds.length) {
      uses.push(Capability.feeds);
      manifest.feeds = feeds.map(
        ([name, feed]): FeedDto => ({
          name,
          description: feed.description ?? "",
          arguments: argumentDtos(feed.arguments),
          fields: Object.entries(feed.fields).map(([field, spec]) => ({ name: field, kind: spec.kind, description: spec.description ?? "" })),
        }),
      );
    }
    manifest.uses = uses;
  }
  return manifest;
}

function variableDto(name: string, spec: VariableSpec): VariableDto {
  return {
    name,
    description: spec.description ?? "",
    kind: spec.kind,
    unit: spec.unit ?? null,
    choices: spec.choices ?? [],
    readOnly: spec.readOnly ?? true,
  };
}

function wrap(kind: Kind, value: Value): ValueDto {
  switch (kind) {
    case "bool":
      return { kind, value: Boolean(value) };
    case "number":
      return { kind, value: Number(value) };
    default:
      return { kind, value: String(value) };
  }
}

function toChoice(suggestion: Suggestion): ChoiceDto {
  return typeof suggestion === "string"
    ? { value: suggestion }
    : suggestion.label && suggestion.label !== suggestion.value
      ? { value: suggestion.value, label: suggestion.label }
      : { value: suggestion.value };
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Define a plugin. See the module documentation. */
export function definePlugin(definition: PluginDefinition): Plugin {
  validate(definition);

  const declared = new Map<string, VariableSpec>(Object.entries(definition.variables ?? {}));
  const published = new Map<string, Value>();
  const sent: Message[] = [];
  let settings: Record<string, string> = {};
  let dataDir: string | null = null;
  // The ABI the agent asked for. Nothing ABI 3 added is sent under 2.
  let abi = ABI_VERSION;
  let write: (message: Message) => void = (message) => {
    sent.push(message);
  };

  const notify = (method: string, params: unknown) => write({ jsonrpc: "2.0", method, params });
  // Which feed each watching key runs.
  const watching = new Map<string, string>();

  const defaults = (): Record<string, string> => {
    const filled: Record<string, string> = {};
    for (const [key, setting] of Object.entries(definition.settings ?? {})) {
      if (setting.default !== undefined) filled[key] = setting.default;
    }
    return filled;
  };

  const publish = (values: Record<string, Value>) => {
    const wire: Record<string, ValueDto> = {};
    for (const [name, value] of Object.entries(values)) {
      const spec = declared.get(name);
      if (!spec) {
        context.log.error(`'${name}' was never declared, so it was not published`);
        continue;
      }
      if (published.get(name) === value) continue;
      published.set(name, value);
      wire[name] = wrap(spec.kind, value);
    }
    return wire;
  };

  const context: Context = {
    id: definition.id,
    get settings() {
      return settings;
    },
    get dataDir() {
      return dataDir;
    },
    vars: {
      set(values) {
        const wire = publish(values);
        if (Object.keys(wire).length > 0) notify(Notify.varsSet, { values: wire });
      },
      declare(variables, options) {
        const prefix = `${definition.id}.`;
        for (const name of Object.keys(variables)) {
          check(name.startsWith(prefix), `variable '${name}' must start with '${prefix}'`);
        }
        if (options?.replace) {
          for (const name of [...declared.keys()]) {
            if (!(name in variables) && !(name in (definition.variables ?? {}))) {
              declared.delete(name);
              published.delete(name);
            }
          }
        }
        for (const [name, spec] of Object.entries(variables)) declared.set(name, spec);
        notify(Notify.varsDeclare, {
          variables: [...declared.entries()].map(([name, spec]) => variableDto(name, spec)),
          replace: options?.replace ?? false,
        });
      },
      get(name) {
        return published.get(name);
      },
    },
    emit(event, payload = {}) {
      if (!(event in (definition.events ?? {}))) {
        context.log.error(`'${event}' is not a declared event`);
        return;
      }
      notify(Notify.eventEmit, { event, payload });
    },
    notify(title, body = "") {
      notify(Notify.notify, { title, body });
    },
    storeSettings(values) {
      const known = definition.settings ?? {};
      for (const key of Object.keys(values)) {
        check(key in known, `setting '${key}' is not declared, so it cannot be stored`);
      }
      check(abi >= 3, "storeSettings needs an agent that speaks plugin ABI 3");
      settings = { ...settings, ...values };
      notify(Notify.settingsStore, { values });
    },
    openUrl(url) {
      check(/^https?:\/\/\S+$/i.test(url), `'${url}' is not an http(s) address`);
      check(abi >= 3, "openUrl needs an agent that speaks plugin ABI 3");
      notify(Notify.openUrl, { url });
    },
    images: {
      put(bytes, mime) {
        check((IMAGE_MIMES as readonly string[]).includes(mime), `'${mime}' is not an image type the agent shows`);
        check(bytes.byteLength <= MAX_IMAGE_BYTES, `the image is ${bytes.byteLength} bytes; the limit is ${MAX_IMAGE_BYTES}`);
        check(abi >= 3, "images.put needs an agent that speaks plugin ABI 3");
        const sha256 = createHash("sha256").update(bytes).digest("hex");
        notify(Notify.imagesPut, { sha256, mime, data: Buffer.from(bytes).toString("base64") });
        return `img:${sha256}`;
      },
    },
    series: {
      set(name, points) {
        check(abi >= 4, "series.set needs an agent that speaks plugin ABI 4");
        check(name.startsWith(`${definition.id}.`), `'${name}' must start with '${definition.id}.'`);
        const finite = points.filter((point) => Number.isFinite(point)).slice(-MAX_SERIES_POINTS);
        notify(Notify.seriesSet, { name, points: finite });
      },
    },
    feed(instance) {
      return {
        set(values) {
          const feed = watching.get(instance);
          check(feed !== undefined, `nothing is watching a feed as '${instance}'`);
          const fields = definition.feeds?.[feed]?.fields ?? {};
          const fresh: Record<string, VariableSpec> = {};
          const full: Record<string, Value> = {};
          for (const [field, value] of Object.entries(values)) {
            const spec = fields[field];
            if (!spec) {
              context.log.error(`'${field}' is not a field of the ${feed} feed`);
              continue;
            }
            const name = `${definition.id}.@${instance}.${field}`;
            if (!declared.has(name)) fresh[name] = { kind: spec.kind, description: spec.description };
            full[name] = value;
          }
          if (Object.keys(fresh).length > 0) context.vars.declare(fresh);
          context.vars.set(full);
        },
      };
    },
    log: {
      info: (message) => notify(Notify.log, { level: "info", message }),
      warn: (message) => notify(Notify.log, { level: "warn", message }),
      error: (message) => notify(Notify.log, { level: "error", message }),
    },
  };

  const handle = async (method: string, params: unknown): Promise<unknown> => {
    const body = (params ?? {}) as Record<string, unknown>;
    switch (method) {
      case Method.initialize: {
        if (typeof body.abi !== "number" || !SUPPORTED_ABIS.includes(body.abi)) {
          throw new Error(`this plugin speaks ABI ${SUPPORTED_ABIS.join(" or ")}, the agent speaks ${String(body.abi)}`);
        }
        abi = body.abi;
        plugin.configure((body.settings as Record<string, string>) ?? {}, (body.dataDir as string | null) ?? null);
        const answer = { manifest: manifestOf(definition, abi), variables: plugin.variables() };
        // Start after the answer is on its way, so a slow connect does not
        // count against the initialize deadline.
        setImmediate(() => {
          Promise.resolve(definition.start?.(context)).catch((error) =>
            context.log.error(`start failed: ${errorMessage(error)}`),
          );
        });
        return answer;
      }
      case Method.variables:
        return plugin.variables();
      case Method.check: {
        const { action, args = {} } = body as { action: string; args?: Record<string, string> };
        const spec = definition.actions[action];
        if (!spec) return { ok: false, error: `no action called '${action}'` };
        const problem = await spec.check?.(args, context);
        return problem ? { ok: false, error: problem } : { ok: true };
      }
      case Method.invoke: {
        const { action, args = {} } = body as { action: string; args?: Record<string, string> };
        const spec = definition.actions[action];
        if (!spec) return { ok: false, error: `no action called '${action}'` };
        try {
          const result = await spec.run(args, context);
          const answer: InvokeResponse = { ok: true };
          if (abi >= 3 && result && typeof result.state === "string") answer.state = result.state;
          return answer;
        } catch (error) {
          return { ok: false, error: errorMessage(error) };
        }
      }
      case Method.sample: {
        const wanted = ((body.wanted as string[]) ?? []).filter((name) => declared.has(name));
        if (wanted.length === 0) return { values: {} };
        const returned = (await definition.sample?.(wanted, context)) ?? {};
        publish(returned);
        const values: Record<string, ValueDto> = {};
        for (const name of wanted) {
          const value = published.get(name);
          const spec = declared.get(name);
          if (value !== undefined && spec) values[name] = wrap(spec.kind, value);
        }
        return { values };
      }
      case Method.choices: {
        const { action, args = {} } = body as { action: string; args?: Record<string, string> };
        const spec = definition.actions[action];
        const answer: ChoicesResponse = { arguments: {} };
        if (!spec) return { ...answer, error: `no action called '${action}'` };
        const asked = (Object.entries(spec.arguments ?? {}) as [string, ArgumentSpec][]).filter(
          ([, argument]) => argument.suggest,
        );
        // Asked together, so one slow lookup does not queue the others; the
        // first failure is the reason given, and the rest still answer.
        // `async` so a suggest that throws synchronously still settles as a
        // rejection rather than escaping `allSettled`.
        const results = await Promise.allSettled(
          asked.map(async ([, argument]) => argument.suggest!(args, context)),
        );
        results.forEach((result, index) => {
          const name = asked[index]![0];
          if (result.status === "fulfilled") {
            if (result.value) answer.arguments[name] = result.value.map(toChoice);
          } else {
            answer.error ??= errorMessage(result.reason);
          }
        });
        return answer;
      }
      case Method.feedWatch: {
        const { instance, feed, args = {} } = body as { instance: string; feed: string; args?: Record<string, string> };
        const spec = definition.feeds?.[feed];
        if (!spec) throw new Error(`no feed called '${feed}'`);
        watching.set(instance, feed);
        const filled: Record<string, string> = {};
        for (const [name, argument] of Object.entries(spec.arguments ?? {}) as [string, ArgumentSpec][]) {
          filled[name] = args[name]?.trim() ? args[name]! : argument.example;
        }
        await spec.watch(instance, filled, context);
        return {};
      }
      case Method.feedUnwatch: {
        const { instance } = body as { instance: string };
        const feed = watching.get(instance);
        watching.delete(instance);
        const prefix = `${definition.id}.@${instance}.`;
        for (const name of [...declared.keys()]) {
          if (name.startsWith(prefix)) {
            declared.delete(name);
            published.delete(name);
          }
        }
        if (feed) await definition.feeds?.[feed]?.unwatch?.(instance, context);
        return {};
      }
      case Method.settingsChanged:
        settings = { ...defaults(), ...((body.settings as Record<string, string>) ?? {}) };
        await definition.settingsChanged?.(context);
        return {};
      case Method.shutdown:
        await definition.stop?.(context);
        setImmediate(() => process.exit(0));
        return {};
      default:
        throw new Error(`no method '${method}'`);
    }
  };

  const plugin: Plugin = {
    definition,
    manifest: (version = ABI_VERSION) => manifestOf(definition, version),
    variables: () => [...declared.entries()].map(([name, spec]) => variableDto(name, spec)),
    handle,
    sent,
    context,
    configure(values, directory = null) {
      settings = { ...defaults(), ...values };
      dataDir = directory;
    },
    useAbi(version) {
      check(SUPPORTED_ABIS.includes(version), `ABI ${version} is not one this SDK speaks`);
      abi = version;
    },
    serve() {
      // stdout is the protocol. Anything a plugin prints by habit goes to
      // stderr, which the agent keeps as the plugin's log.
      console.log = console.error;
      console.info = console.error;
      write = (message) => {
        process.stdout.write(`${JSON.stringify(message)}\n`);
      };
      const lines = createInterface({ input: process.stdin });
      lines.on("line", (line) => {
        if (!line.trim()) return;
        let message: Message;
        try {
          message = JSON.parse(line) as Message;
        } catch {
          context.log.error(`the agent sent something that is not JSON: ${line.slice(0, 200)}`);
          return;
        }
        if (message.id === undefined || !message.method) return;
        const id = message.id;
        handle(message.method, message.params).then(
          (result) => write({ jsonrpc: "2.0", id, result: result ?? {} }),
          (error) => write({ jsonrpc: "2.0", id, error: { code: -32000, message: errorMessage(error) } }),
        );
      });
      // The agent closing our stdin means it is gone; follow it.
      lines.on("close", () => {
        Promise.resolve(definition.stop?.(context)).finally(() => process.exit(0));
      });
    },
  };

  if (process.env.FLEXDECK_PLUGIN_ID === definition.id) {
    queueMicrotask(() => plugin.serve());
  }
  return plugin;
}
