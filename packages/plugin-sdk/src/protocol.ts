/**
 * The wire contract between the flexdeck agent and a plugin.
 *
 * These types mirror `crates/flexdeck-plugin-abi` exactly. They are kept by hand
 * rather than generated, and pinned by a shared fixture
 * (`contract/fixtures/plugin/manifest.json`): the SDK's tests produce it from a
 * definition, and the Rust ABI's tests parse it with unknown fields refused. A
 * field added on one side and not the other fails one of the two suites.
 */

/** The newest contract version. Must equal `ABI_VERSION` in the Rust crate. */
export const ABI_VERSION = 4;

/**
 * Every version this SDK can answer in. The agent asks for the one the
 * package's `plugin.json` names, so an older package keeps working; at ABI 2
 * the fields ABI 3 added are left out of the answer.
 */
export const SUPPORTED_ABIS: readonly number[] = [2, 3, 4];

/** ABI 4: what a host offers and a plugin uses, named in `initialize` and the manifest. */
export const Capability = {
  series: "series",
  presets: "presets",
  feeds: "feeds",
} as const;
export type Capability = (typeof Capability)[keyof typeof Capability];

/** ABI 4: the most points one `series/set` carries. */
export const MAX_SERIES_POINTS = 600;

export type Kind = "text" | "number" | "bool" | "choice";

export type Permission = "network" | "notifications" | "processes" | "filesystem";

export interface ArgumentDto {
  name: string;
  description: string;
  example: string;
  kind: Kind;
  choices: string[];
  optional: boolean;
}

export interface ActionDto {
  action: string;
  description: string;
  arguments: ArgumentDto[];
  /** ABI 3: a slider can drive this action. */
  slider?: SliderDto;
}

/** ABI 3: which argument a slider writes, its range, and the variable it follows. */
export interface SliderDto {
  argument: string;
  min: number;
  max: number;
  step: number;
  feedback?: string;
  /** How the same channel is muted: a toggle action and its muted variable. */
  mute?: { action: string; feedback?: string };
}

export interface SettingDto {
  key: string;
  description: string;
  kind: Kind;
  choices: string[];
  defaultValue: string | null;
  secret: boolean;
}

export interface VariableDto {
  name: string;
  description: string;
  kind: Kind;
  unit: string | null;
  choices: string[];
  readOnly: boolean;
}

export interface EventDto {
  name: string;
  description: string;
  fields: string[];
}

export interface ManifestDto {
  abi: number;
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  actions: ActionDto[];
  settings: SettingDto[];
  events: EventDto[];
  permissions: Permission[];
  /** ABI 4: the capabilities it uses. */
  uses?: Capability[];
  /** ABI 4: ready-made keys for the gallery. */
  presets?: PresetDto[];
  /** ABI 4: values per key. */
  feeds?: FeedDto[];
}

/** ABI 4: a ready-made key; `widget` is a widget in the configuration's JSON, without id or place. */
export interface PresetDto {
  id: string;
  name: string;
  category: string;
  description: string;
  w: number;
  h: number;
  widget: Record<string, unknown>;
}

/** ABI 4: values a key gets for itself, with its own arguments. */
export interface FeedDto {
  name: string;
  description: string;
  arguments: ArgumentDto[];
  fields: { name: string; kind: Kind; description: string }[];
}

/** `plugin.json`, at the root of a package. */
export interface PackageManifest {
  abi: number;
  id: string;
  name: string;
  version: string;
  description?: string;
  author?: string;
  homepage?: string | null;
  /** `recipe` (ABI 4): no program; `recipe` names a JSON file of web sources and presets. */
  runtime: "node" | "native" | "recipe" | "python";
  recipe?: string;
  entry?: string | null;
  entries?: Record<string, string>;
  permissions?: Permission[];
}

export interface ValueDto {
  kind: Kind;
  value: boolean | number | string;
}

/** Methods the host calls. */
export const Method = {
  initialize: "initialize",
  variables: "variables",
  check: "check",
  invoke: "invoke",
  sample: "sample",
  settingsChanged: "settings/changed",
  shutdown: "shutdown",
  /** What an action's arguments can be set to right now. Optional for a plugin. */
  choices: "choices",
  /** ABI 4: a key started using a feed. */
  feedWatch: "feed/watch",
  /** ABI 4: that key is gone. */
  feedUnwatch: "feed/unwatch",
} as const;

/** One value an argument can take, and how the editor shows it. */
export interface ChoiceDto {
  value: string;
  label?: string;
}

/** The answer to `choices`: suggestions per argument, and why if there are none. */
export interface ChoicesResponse {
  arguments: Record<string, ChoiceDto[]>;
  error?: string;
}

/** Notifications a plugin may send at any time. */
export const Notify = {
  varsSet: "vars/set",
  varsDeclare: "vars/declare",
  eventEmit: "event/emit",
  log: "log",
  notify: "notify",
  /** ABI 3: persist values for declared settings (a refresh token, say). */
  settingsStore: "settings/store",
  /** ABI 3: open an http(s) address in the person's browser. */
  openUrl: "host/open_url",
  /** ABI 3: hand over an image a widget can show as `img:<sha256>`. */
  imagesPut: "images/put",
  /** ABI 4: a variable's whole recent history, for a graph. */
  seriesSet: "series/set",
} as const;

/** ABI 3: the largest image `images/put` accepts, decoded. */
export const MAX_IMAGE_BYTES = 256 * 1024;

/** ABI 3: the image types `images/put` accepts. */
export const IMAGE_MIMES = ["image/png", "image/jpeg", "image/webp"] as const;
export type ImageMime = (typeof IMAGE_MIMES)[number];

/** The answer to `invoke`. `state` (ABI 3) is the look a widget should show. */
export interface InvokeResponse {
  ok: boolean;
  error?: string;
  state?: string;
}

export interface Message {
  jsonrpc: "2.0";
  id?: number;
  method?: string;
  params?: unknown;
  result?: unknown;
  error?: { code: number; message: string };
}

/** Ids become variable namespaces, directory names and URL segments. */
export function isValidId(id: string): boolean {
  return /^[a-z][a-z0-9_-]{0,63}$/.test(id);
}
