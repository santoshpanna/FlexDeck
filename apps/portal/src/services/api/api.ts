/**
 * The portal's client: the cloud, and the desktops this browser can reach.
 *
 * ---------------------------------------------------------------------------
 * TWO WAYS TO A DESKTOP, ONE VOCABULARY
 * ---------------------------------------------------------------------------
 * A desktop is reached directly when the portal is open on the same computer
 * (`http://127.0.0.1`, the way a keyboard vendor's web configurator reaches the
 * keyboard on your desk), or through the cloud, which forwards the call down the
 * desktop's tunnel. Either way the request is the desktop's own REST API —
 * `config()`, `putConfig()`, `status()` — so the methods below do not care which;
 * only [`Api.relay`] does.
 *
 * ---------------------------------------------------------------------------
 * THERE ARE NO ACCOUNTS
 * ---------------------------------------------------------------------------
 * The credential for a desktop is a token THAT DESKTOP issued this browser, kept
 * in this browser (see `connections.lib.ts`) and checked by the desktop. The
 * cloud passes it through and holds nothing of its own, so there is no session
 * cookie and nothing here for a cloud-side bug to leak across users.
 */

import type { Background, Face } from "@flexdeck/face";
import { LOCAL_AGENT, type Connection } from "../connections/connections.lib.ts";
import { LiveFeed, type LiveListener } from "./live.ts";

export interface Status {
  rev: number | null;
  fingerprint: string;
  host: string;
  port: number;
  deviceCount: number;
}

export interface Device {
  id: string;
  name: string;
  created_at: number;
}

export interface PairingInfo {
  version: string;
  host: string;
  port: number;
  fingerprint: string;
  sessionId: string;
  oneTimeSecret: string;
  expiresAt: string;
}

export interface PairingBegin {
  pin: string;
  expiresAt: number;
  qr: PairingInfo;
}

/**
 * Configuration types as they arrive over the wire.
 *
 * Every optional field is genuinely optional: this is the proto3 JSON mapping,
 * which *omits* default values. `gapX: 0` is absent, not zero, so a client that
 * assumes presence reads `undefined` and computes nonsense.
 */
export interface WireAction {
  plugin?: string;
  action?: string;
  args?: Record<string, string>;
}

/** The configuration schema this portal writes. 3 adds sliders and holds; 4 faces. */
export const CONFIG_SCHEMA = 4;

/** How a person touched a widget. Each has at most one flow on a widget. */
export type Gesture =
  | "GESTURE_SHORT_PRESS"
  | "GESTURE_LONG_PRESS"
  | "GESTURE_DOUBLE_TAP"
  /** A finger went down and stays down: push-to-talk. */
  | "GESTURE_HOLD_START"
  /** …and lifted (or the tablet went away). */
  | "GESTURE_HOLD_END";

/**
 * What starts a flow: a gesture, a plugin announcing an event, or — on a
 * slider — the slider settling on a value.
 */
export interface WireFlowTrigger {
  gesture?: Gesture;
  event?: { plugin?: string; event?: string; filter?: Record<string, string> };
  value?: Record<string, never>;
}

export type SliderOrientation =
  | "SLIDER_ORIENTATION_AUTO"
  | "SLIDER_ORIENTATION_HORIZONTAL"
  | "SLIDER_ORIENTATION_VERTICAL";

/** A slider's range, direction, and the value its thumb follows. */
export interface WireSlider {
  orientation?: SliderOrientation;
  min?: number;
  max?: number;
  step?: number;
  variable?: string;
  throttleMs?: number;
}

/** A widget's run of steps, in the automation engine's block language. */
export interface WireFlow {
  trigger?: WireFlowTrigger;
  /** Tells several event-triggered flows apart. */
  name?: string;
  blocks?: Block[];
  /** A press on this zone of the face's strip runs it, not one on the key. */
  zone?: string;
}

/** One look of a multi-state widget. Empty fields fall back to its props. */
export interface WireLook {
  id: string;
  label?: string;
  bgColor?: string;
  fgColor?: string;
  icon?: string;
  /** This look's whole face (schema 4). */
  face?: Face;
}

/** How a case compares the value with its own: equal unless it says. */
export type CaseOp =
  | "COMPARE_OP_EQ"
  | "COMPARE_OP_NE"
  | "COMPARE_OP_LT"
  | "COMPARE_OP_LE"
  | "COMPARE_OP_GT"
  | "COMPARE_OP_GE";

export interface WireCase {
  value?: string;
  state?: string;
  op?: CaseOp;
}

/** A widget's looks, and the one rule that picks which shows. */
export interface WireStates {
  looks?: WireLook[];
  cycle?: Record<string, never>;
  variable?: { variable?: string; cases?: WireCase[]; otherwise?: string };
  provider?: { flow?: number; path?: number[] };
}

// ---------------------------------------------------------------------------
// AUTOMATIONS — the reactive layer's wire shape.
//
// These mirror `flexdeck-automation`'s serde rename rules:
//   * `Block` is a tagged enum with `block` as the tag (verb the editor draws).
//   * `Condition` is a tagged enum with `kind` as the tag (NOT `condition` —
//     that collided with `Not.condition`).
//   * `Trigger` is a tagged enum with `on` as the tag.
//   * `ValueRef` is externally tagged: `{"variable":"name"}`, `{"number":50}`.
//
// The editor renders these without re-typing by walking the discriminant; the
// `block`/`on`/`kind` shapes are the contract.
// ---------------------------------------------------------------------------

export type ValueRef =
  | { variable: string }
  | { bool: boolean }
  | { number: number }
  | { text: string }
  /** A value the run was handed: a slider's `value`, an event's payload field. */
  | { input: string };

export type CompareOp =
  | "eq"
  | "notEq"
  | "greater"
  | "greaterOrEqual"
  | "less"
  | "lessOrEqual"
  | "contains";

export type Condition =
  | { kind: "compare"; left: ValueRef; op: CompareOp; right: ValueRef }
  | { kind: "all"; conditions: Condition[] }
  | { kind: "any"; conditions: Condition[] }
  | { kind: "not"; condition: Condition };

export type Trigger =
  | { on: "variables"; names: string[] }
  | { on: "widget"; widgetId: string }
  | { on: "manual" }
  /** A plugin announced an event; every `filter` key must match its payload. */
  | { on: "pluginEvent"; plugin: string; event: string; filter: Record<string, string> }
  /** Every `everySeconds`, or daily at `at` ("HH:MM") on `days` (empty = every day). */
  | { on: "schedule"; everySeconds?: number; at?: string; days?: string[] };

export type Block =
  | {
      block: "if";
      condition: Condition;
      then: Block[];
      elseIf?: { condition: Condition; blocks: Block[] }[];
      otherwise?: Block[];
    }
  | { block: "repeat"; times: ValueRef; body: Block[] }
  | { block: "wait"; ms: ValueRef }
  | { block: "setVariable"; name: string; value: ValueRef }
  | { block: "doAction"; plugin: string; action: string; args: Record<string, ValueRef> };

export interface Automation {
  id: string;
  name: string;
  enabled?: boolean;
  trigger: Trigger;
  body: Block[];
}

export interface AutomationDocument {
  schema?: number;
  rev?: number;
  automations?: Automation[];
}

/** The trace of one dry-run, exactly as the editor renders it. */
export type Trace =
  | { kind: "condition"; summary: string; outcome: ConditionOutcome }
  | { kind: "waited"; ms: number }
  | { kind: "repeated"; times: number }
  | { kind: "set"; name: string; value: ValueRef }
  | { kind: "ran"; plugin: string; action: string; input?: string; state?: string }
  | { kind: "stopped"; reason: string };

export type ConditionOutcome =
  | { kind: "true" }
  | { kind: "false" }
  | { kind: "unknown"; reason: string };

export type RunOutcome =
  | "completed"
  | "cancelled"
  | "limitReached"
  | "failed";

export interface Run {
  outcome: RunOutcome;
  trace: Trace[];
}

export interface WireWidget {
  id: string;
  /** Where it sits in portrait once someone put it there. Absent: assumed (`placePage`). */
  portrait?: WirePlacement;
  type?: string;
  col?: number;
  row?: number;
  w?: number;
  h?: number;
  props?: {
    label?: string;
    icon?: string;
    bgColor?: string;
    fgColor?: string;
    /** A live label: "REC {{obs.record_time}}". */
    labelTemplate?: string;
    /** A live icon: "{{media.spotify.art}}" shows the album art. */
    iconTemplate?: string;
  };
  /** What the widget does: one flow per gesture, and any event-triggered ones. */
  flows?: WireFlow[];
  /** Several looks and what picks one. Absent: one look, its props. */
  states?: WireStates;
  /** For a `WIDGET_TYPE_SLIDER`. */
  slider?: WireSlider;
  /** Its own face (schema 4). Absent: an icon over a label, from `props`. */
  face?: Face;
  /** Data sources it runs its own copy of; `{{this.x}}` reads the first. */
  sources?: { source: string; args?: Record<string, string> }[];
}

export interface IconPackSummary {
  id: string;
  name: string;
  author?: string;
  count: number;
}

/** One pack icon: a picture, or paths on a view box. */
export interface IconPackIcon {
  name: string;
  tags?: string[];
  image?: string;
  paths?: string[];
  view?: [number, number, number, number];
  fill?: boolean;
  stroke?: number;
}

export interface IconPack extends IconPackSummary {
  icons: IconPackIcon[];
}

/** Read a file the person chose as base64, for an upload. */
export function fileAsBase64(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).replace(/^data:[^,]*,/, ""));
    reader.onerror = () => reject(reader.error ?? new Error("the file could not be read"));
    reader.readAsDataURL(file);
  });
}

/** How the desktop writes numbers, money, times and temperatures. */
export interface DesktopLocale {
  decimal: string;
  group: string;
  currency: string;
  hour12: boolean;
  fahrenheit: boolean;
}

/** A data feed: a URL or a program, and the fields read from what comes back. */
export interface DataSource {
  id: string;
  name?: string;
  enabled?: boolean;
  kind?: "http" | "command";
  everySeconds?: number;
  http?: { url: string; method?: string; headers?: Record<string, string>; body?: string };
  command?: { program: string; args?: string[] };
  fields?: { name: string; value: string; series?: boolean }[];
  /** `secret`: a token or key, kept on the computer only — never in the config. */
  args?: { name: string; default?: string; description?: string; secret?: boolean }[];
  /** May reach this computer and this network. */
  lan?: boolean;
}

export interface SourceStatus {
  ok: boolean;
  error?: string | null;
  updated: number;
}

/** What one test run of a source got back. */
export interface SourceTest {
  status?: number;
  text?: string;
  json?: unknown;
  values?: Record<string, number | string | boolean | null>;
  series?: Record<string, number[]>;
  error?: string;
}

export interface WirePage {
  id: string;
  name?: string;
  widgets?: WireWidget[];
  /** Behind every key: a picture or a gradient across the page (schema 4). */
  background?: Background;
}

/** What the agent says a widget shows now (`StateEvent`), for the preview. */
export interface WidgetPreview {
  state?: {
    stateId?: string;
    label?: string;
    icon?: string;
    elements?: { index?: number; text?: string; number?: number; color?: string; pulse?: boolean }[];
    background?: string;
    pulse?: boolean;
    backgroundImage?: string;
    /** Why it may be stale: a source it reads is failing. */
    problem?: string;
    /** Zones of its strip whose `when` holds: they show their other icon. */
    zonesOn?: string[];
  };
  /** Why it cannot work, in the agent's words. */
  error?: string;
}

export interface WireGrid {
  cols?: number;
  rows?: number;
  gapX?: number;
  gapY?: number;
  usableW?: number;
  usableH?: number;
}

export interface WireProfile {
  id: string;
  name?: string;
  /** The landscape grid: what widgets' own col/row/w/h are placed on. */
  grid?: WireGrid;
  pages?: WirePage[];
  /** Portrait's own grid. Absent: the landscape grid turned on its side. */
  portraitGrid?: WireGrid;
  /** Burn-in shift and night dimming (schema 4). */
  screen?: ScreenCare;
}

/** Looking after a tablet that shows the deck all day. Times are minutes after midnight. */
export interface ScreenCare {
  shift?: boolean;
  dim?: boolean;
  dimFrom?: number;
  dimTo?: number;
  dimLevel?: number;
}

/** A widget's cells on one orientation's grid. Zero fields are omitted on the wire. */
export interface WirePlacement {
  col?: number;
  row?: number;
  w?: number;
  h?: number;
}

export interface WireConfig {
  schema?: number;
  rev?: number;
  profiles?: WireProfile[];
}

/**
 * One action the agent can perform, as described by `/api/plugins`.
 *
 * Fetched rather than hard-coded: the agent owns what its plugins can do, so
 * an action added there appears in this form with no portal change. The
 * `plugin` field distinguishes between built-ins (`core`, `system`,
 * `notifications`) and any future third-party plugins.
 */
export interface ActionArgument {
  name: string;
  description: string;
  example: string;
  /**
   * The input shape the editor should render for this argument.
   *
   * `text` and `number` both render a text box (with `number` getting the
   * numeric stepper); `bool` renders a true/false picker; `choice` renders a
   * dropdown populated from `choices`. Anything else falls through to a plain
   * text box so a plugin published tomorrow still renders.
   */
  kind?: "text" | "number" | "bool" | "choice";
  /** Allowed values, present only when `kind === "choice"`. */
  choices?: string[];
  /** May be left blank. */
  optional?: boolean;
}

export interface ActionDefinition {
  plugin: string;
  /** The plugin's name as a person reads it ("OBS Studio"), when known. */
  pluginName?: string;
  action: string;
  description: string;
  arguments: ActionArgument[];
  /** Set when a slider can drive this action. */
  slider?: SliderTarget;
  /** A key running it is lit while `variable` equals the `equals` argument. */
  active?: { variable: string; equals: string };
}

/**
 * How a slider drives an action: the argument its value goes into, the range,
 * and the variable (with `{{argument}}` placeholders) that reports it back.
 */
export interface SliderTarget {
  argument: string;
  min: number;
  max: number;
  step: number;
  feedback?: string;
  /**
   * How the same channel is muted: an action of the same plugin that toggles
   * it, taking the slider action's other arguments, and the variable (with
   * placeholders) saying whether it is.
   */
  mute?: { action: string; feedback?: string };
}

/**
 * One entry in the plugin catalogue, exactly as `GET /api/plugins` returns it.
 *
 * Only the fields the editor cares about are typed here — `version`,
 * `description` of the plugin itself, and the per-plugin `variables` and
 * `settings` arrays are read by other parts of the editor (the variables panel,
 * the plugin settings dialog) but not by this hook.
 */
export interface PluginListing {
  id: string;
  name: string;
  version?: string;
  description?: string;
  origin?: "builtIn" | "installed";
  status?: string | null;
  actions: ActionDefinition[];
  events?: PluginEvent[];
  settings?: PluginSetting[];
  variables?: VariableChoice[];
  /** Ready-made keys for the gallery (plugin ABI 4). */
  presets?: { id: string; name: string; category?: string; description?: string; w?: number; h?: number; widget: Record<string, unknown> }[];
  /** Values a key gets for itself (plugin ABI 4). */
  feeds?: {
    name: string;
    description?: string;
    arguments?: { name: string; description?: string; example?: string }[];
    fields?: { name: string; kind: string; description?: string }[];
  }[];
}

/** A live value a plugin owns, for binding a widget or a trigger to it. */
export interface VariableChoice {
  name: string;
  kind: "text" | "number" | "bool" | "choice";
  description: string;
  /** Only its plugin may change it: a step cannot set it. */
  readOnly?: boolean;
}

/** One event any plugin on a desktop can announce, flattened for a picker. */
export interface PluginEventChoice {
  plugin: string;
  pluginName: string;
  event: string;
  description: string;
  fields: string[];
}

/** Something a plugin can announce, which an automation can trigger on. */
export interface PluginEvent {
  name: string;
  description: string;
  fields: string[];
}

/** One field of a plugin's settings form. A secret's value never comes back. */
export interface PluginSetting {
  key: string;
  description: string;
  kind: "text" | "number" | "bool" | "choice";
  choices: string[];
  defaultValue: string | null;
  secret: boolean;
  value: string | null;
  isSet: boolean;
}

/** A plugin installed on a desktop, with its live state. */
export interface InstalledPlugin {
  id: string;
  name: string;
  version: string;
  permissions: string[];
  enabled: boolean;
  source: { kind: string; path?: string; url?: string };
  running: boolean;
  status: string | null;
  error: string | null;
}

/** Which list a plugin repository is on. */
export type RepoKind = "official" | "vetted" | "custom";

/** One plugin in the marketplace: a repository's entry, as the desktop merged it. */
export interface CatalogEntry {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  homepage?: string;
  abi: number;
  permissions: string[];
  releaseDate?: string;
  latestUpdateDate?: string;
  package: { url: string; sha256: string; size: number };
  repo: { url: string; name: string; kind: RepoKind };
  /** The icon as a `data:` URI; the desktop fetched it, so this page never does. */
  iconData: string | null;
  installedVersion: string | null;
  installedFromHere: boolean;
  update: boolean;
}

/** A plugin repository the desktop reads. */
export interface Repository {
  url: string;
  name: string;
  description: string;
  website: string | null;
  kind: RepoKind;
  enabled: boolean;
  pluginCount: number;
  /** Seconds since the epoch. */
  fetchedAt: number | null;
  error: string | null;
  dropped: string[];
}

/** What adding a repository would add. */
export interface RepoPreview {
  url: string;
  name: string;
  description: string;
  website: string | null;
  pluginCount: number;
  plugins: string[];
  kind: RepoKind;
}

export interface Catalog {
  plugins: CatalogEntry[];
  repositories: Repository[];
  vettedError: string | null;
}

/** What an install answered: done, or "these permissions need agreeing to". */
/** One value an action's argument can take, as its plugin offers it. */
export interface ArgumentChoice {
  value: string;
  label?: string;
}

/** What a plugin suggests for an action's arguments, and why when it cannot. */
export interface ArgumentChoices {
  arguments: Record<string, ArgumentChoice[]>;
  error?: string;
}

/** What a package file is, as the desktop read it, before anything is installed. */
export interface InspectedPackage {
  package: {
    id: string;
    name: string;
    version: string;
    description?: string;
    author?: string;
    homepage?: string | null;
    permissions?: string[];
  };
  sha256: string;
  size: number;
  installedVersion: string | null;
}

/** The largest package the desktop accepts. */
export const MAX_PACKAGE_BYTES = 64 * 1024 * 1024;
/**
 * The largest package that fits through the cloud relay: the desktop takes a
 * relayed request of up to 1 MiB, and base64 makes a package a third bigger.
 */
export const MAX_RELAYED_PACKAGE_BYTES = 740 * 1024;

export type InstallAnswer =
  | { installed: InstalledPlugin }
  | { needsConsent: true; missing: string[]; package: { permissions?: string[]; name: string; version: string } };

/** A desktop this browser can reach. */
export interface DesktopSummary {
  /** The fingerprint: the desktop's identity, and how the API addresses it. */
  id: string;
  name: string;
  fingerprint: string;
  /** Reachable right now, by the way it would be reached. */
  online: boolean;
  /** How it is reached: directly on this computer, or through the cloud. */
  via: "local" | "relay";
}

/** What a desktop hands a browser it has just let in. */
export interface PairResult {
  token: string;
  fingerprint: string;
  name: string | null;
}

export class ApiError extends Error {
  /// Declared and assigned explicitly rather than as a constructor *parameter
  /// property*, because Node's type-stripping runtime refuses that syntax —
  /// `node --test` fails to even parse the file. Vite tolerates it, so the only
  /// way to notice is to run the tests, which is exactly the wrong time.
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }

  /**
   * Whether the caller needs to sign in.
   *
   * A getter rather than a field so a caller cannot forget to check it, and so
   * the one place that decides is here rather than repeated at every call site.
   */
  get isUnauthenticated(): boolean {
    return this.status === 401;
  }
}

/** Whether two routes reach the same desktop the same way, with the same token. */
function sameRoute(a: Connection, b: Connection): boolean {
  return a.via === b.via && a.token === b.token && a.fingerprint === b.fingerprint;
}

export class Api {
  // `#` fields rather than constructor parameter properties: Node's
  // type-stripping runtime refuses the latter outright, so the tests could not
  // even parse this file.
  readonly #base: string;
  readonly #local: string;
  /** How to reach each desktop, by fingerprint. Set by the desktop provider. */
  #routes = new Map<string, Connection>();
  /** Told when the desktop on this computer refuses this browser's token. */
  #onLocalRefused: ((desktopId: string) => void) | null = null;
  /**
   * Pictures already fetched, as object URLs, by desktop and sha. A picture's
   * name is its hash, so an entry never goes stale.
   */
  #images = new Map<string, Promise<string | null>>();
  /** Icon packs already fetched, by desktop and pack. */
  #packs = new Map<string, Promise<IconPack | null>>();
  /** One live socket per desktop, open while anything listens. */
  #live = new Map<string, LiveFeed>();

  /**
   * @param base   origin to prefix cloud requests with. Empty means "this
   *               origin", which is where the cloud serves the portal from.
   * @param local  where the desktop app on this computer listens for the page.
   */
  constructor(base = "", local: string = LOCAL_AGENT) {
    this.#base = base;
    this.#local = local;
  }

  /** Which connection to use for each desktop. */
  setRoutes(routes: Map<string, Connection>): void {
    this.#routes = routes;
    // A route that changed — a new token, local instead of the cloud — is a
    // different socket; listeners move to it.
    for (const [desktopId, feed] of [...this.#live]) {
      const route = routes.get(desktopId);
      if (route && sameRoute(route, feed.route)) continue;
      feed.close();
      this.#live.delete(desktopId);
      if (!route) continue;
      const moved = this.#feed(desktopId, route);
      for (const listener of feed.listeners()) moved.listen(listener);
    }
  }

  /**
   * Hear what happens on a desktop as it happens: looks, pages, devices, the
   * configuration moving. One socket per desktop, however many listen, closed
   * when the last stops. Returns the way to stop.
   */
  live(desktopId: string, listener: LiveListener): () => void {
    const route = this.#routes.get(desktopId);
    if (!route) return () => {};
    (this.#live.get(desktopId) ?? this.#feed(desktopId, route)).listen(listener);
    // Through whichever socket is current: a route change moves listeners.
    return () => {
      const feed = this.#live.get(desktopId);
      if (!feed) return;
      feed.unlisten(listener);
      if (!feed.listening) {
        feed.close();
        this.#live.delete(desktopId);
      }
    };
  }

  #feed(desktopId: string, route: Connection): LiveFeed {
    const feed = new LiveFeed(route, this.#base, this.#local, () => {
      if (route.via === "local") this.#onLocalRefused?.(desktopId);
    });
    this.#live.set(desktopId, feed);
    return feed;
  }

  /**
   * Be told when a desktop on this computer refuses this browser's token.
   *
   * It was removed in the desktop app, so it will never work again — and the
   * only remedy is to connect afresh, which only the desktop provider can do.
   */
  setOnLocalRefused(listener: ((desktopId: string) => void) | null): void {
    this.#onLocalRefused = listener;
  }

  private async request<T>(method: string, path: string, body?: unknown, token?: string): Promise<T> {
    const headers: Record<string, string> = {};
    if (body !== undefined) {
      headers["content-type"] = "application/json";
    }
    if (token) {
      headers.authorization = `Bearer ${token}`;
    }
    const response = await fetch(`${this.#base}${path}`, {
      method,
      // No cookies: there is no session, and none should be sent anywhere.
      credentials: "omit",
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });

    const text = await response.text();
    const parsed: unknown = text ? JSON.parse(text) : null;

    if (!response.ok) {
      const message = extractError(parsed) ?? `${response.status} ${response.statusText}`;
      throw new ApiError(response.status, message);
    }

    return parsed as T;
  }

  // -- finding desktops ----------------------------------------------------

  /**
   * The desktop app on this computer, if there is one. Never throws: no answer
   * is the ordinary case on a device without the app.
   */
  async localHello(): Promise<{ fingerprint: string; name: string } | null> {
    try {
      const response = await fetch(`${this.#local}/api/local/hello`, {
        credentials: "omit",
        signal: AbortSignal.timeout(1500),
      });
      return response.ok ? ((await response.json()) as { fingerprint: string; name: string }) : null;
    } catch {
      return null;
    }
  }

  /** Ask the desktop app on this computer to let this page in. */
  async localConnect(name: string): Promise<PairResult> {
    const response = await fetch(`${this.#local}/api/local/connect`, {
      method: "POST",
      credentials: "omit",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name }),
    });
    const parsed: unknown = await response.json().catch(() => null);
    if (!response.ok) {
      throw new ApiError(response.status, extractError(parsed) ?? "the desktop app refused");
    }
    return parsed as PairResult;
  }

  /**
   * Pair with whichever desktop is showing `code`, through the cloud.
   *
   * The desktop checks the code and issues the token; the cloud only finds it.
   */
  pair(code: string, name: string): Promise<PairResult> {
    return this.request("POST", "/api/pair", { code, name });
  }

  /** Which of these desktops are connected to the cloud right now. */
  presence(
    fingerprints: string[],
  ): Promise<{ desktops: { fingerprint: string; online: boolean; name: string | null }[] }> {
    return this.request("POST", "/api/presence", { fingerprints });
  }

  // -- one desktop's own API -----------------------------------------------

  /**
   * Call a desktop's API, directly on this computer or through the cloud.
   *
   * The response is passed through untouched, status included, so a caller sees
   * exactly what the desktop decided. Refusals become errors with the desktop's
   * own words.
   */
  private async relay<T>(
    desktopId: string,
    method: string,
    path: string,
    body?: unknown,
  ): Promise<T> {
    const route = this.#routes.get(desktopId);
    if (!route) {
      throw new ApiError(401, "this browser is not connected to that desktop");
    }
    const text = body === undefined ? "" : JSON.stringify(body);

    let status: number;
    let answer: string;
    if (route.via === "local") {
      const headers: Record<string, string> = { authorization: `Bearer ${route.token}` };
      if (text) {
        headers["content-type"] = "application/json";
      }
      const response = await fetch(`${this.#local}${path}`, {
        method,
        credentials: "omit",
        headers,
        body: text || undefined,
      });
      status = response.status;
      answer = await response.text();
      if (status === 401) {
        this.#onLocalRefused?.(desktopId);
      }
    } else {
      const relayed = await this.request<{ status: number; body: string }>(
        "POST",
        "/api/relay",
        { fingerprint: route.fingerprint, method, path, body: text },
        route.token,
      );
      status = relayed.status;
      answer = relayed.body;
    }

    const parsed: unknown = answer ? JSON.parse(answer) : null;

    if (status === 409 && isConsent(parsed)) {
      throw new ConsentError(parsed);
    }
    if (status < 200 || status >= 300) {
      throw new ApiError(status, extractError(parsed) ?? `the desktop refused (${status})`);
    }

    return parsed as T;
  }

  /**
   * A picture a plugin handed the desktop (`img:<sha256>`), as a URL an `<img>`
   * can show, or `null` where it cannot be had.
   *
   * Fetched with the token rather than linked, since the route is
   * authenticated. Only on this computer: the cloud relay carries text bodies,
   * so through it the answer is `null` and the caller shows a stand-in.
   */
  imageUrl(desktopId: string, sha: string): Promise<string | null> {
    const route = this.#routes.get(desktopId);
    if (!route || route.via !== "local" || !/^[0-9a-f]{64}$/.test(sha)) {
      return Promise.resolve(null);
    }
    const key = `${desktopId}/${sha}`;
    let pending = this.#images.get(key);
    if (!pending) {
      pending = fetch(`${this.#local}/api/images/${sha}`, {
        credentials: "omit",
        headers: { authorization: `Bearer ${route.token}` },
      })
        .then(async (response) => (response.ok ? URL.createObjectURL(await response.blob()) : null))
        .catch(() => null);
      // A miss is not remembered: the plugin may hand the picture over later.
      pending.then((url) => {
        if (url === null) {
          this.#images.delete(key);
        }
      });
      this.#images.set(key, pending);
    }
    return pending;
  }

  /** Whether a call to this desktop goes through the cloud (and so may need a yes at the desktop). */
  isRemote(desktopId: string): boolean {
    return this.#routes.get(desktopId)?.via === "relay";
  }

  status(desktopId: string): Promise<Status> {
    return this.relay(desktopId, "GET", "/api/status");
  }

  devices(desktopId: string): Promise<{ devices: Device[] }> {
    return this.relay(desktopId, "GET", "/api/devices");
  }

  /**
   * The plugin catalogue, with every action of every plugin flattened into one
   * list. The editor's `ActionForm` was built around this flat shape, so we
   * keep it stable and do the flattening here — adding a plugin becomes visible
   * in the dropdown with no change to the form.
   */
  async plugins(
    desktopId: string,
  ): Promise<{ actions: ActionDefinition[]; events: PluginEventChoice[]; variables: VariableChoice[] }> {
    const answer = await this.relay<{ plugins: PluginListing[] }>(
      desktopId,
      "GET",
      "/api/plugins",
    );
    const actions: ActionDefinition[] = [];
    const events: PluginEventChoice[] = [];
    const variables: VariableChoice[] = [];
    for (const plugin of answer.plugins) {
      for (const variable of plugin.variables ?? []) {
        variables.push({
          name: variable.name,
          kind: variable.kind,
          description: variable.description,
          ...(variable.readOnly ? { readOnly: true } : {}),
        });
      }
      for (const event of plugin.events ?? []) {
        events.push({
          plugin: plugin.id,
          pluginName: plugin.name,
          event: event.name,
          description: event.description,
          fields: event.fields,
        });
      }
      for (const action of plugin.actions) {
        actions.push({
          plugin: plugin.id,
          pluginName: plugin.name,
          action: action.action,
          description: action.description,
          arguments: action.arguments,
          ...(action.slider ? { slider: action.slider } : {}),
          ...(action.active ? { active: action.active } : {}),
        });
      }
    }
    // Values no plugin declares up front — the person's own (`vars.`) and the
    // data sources' (`src.`) — come from the live list.
    try {
      const live = await this.relay<{ spec?: { name?: string; kind?: string; description?: string; readOnly?: boolean } }[]>(desktopId, "GET", "/api/variables");
      const known = new Set(variables.map((variable) => variable.name));
      for (const entry of live ?? []) {
        const spec = entry.spec;
        const name = spec?.name ?? "";
        if (!name || known.has(name) || !(name.startsWith("vars.") || name.startsWith("src."))) continue;
        const kind = spec?.kind === "number" || spec?.kind === "bool" || spec?.kind === "choice" ? spec.kind : "text";
        variables.push({ name, kind, description: spec?.description ?? "", ...(spec?.readOnly ? { readOnly: true } : {}) });
      }
    } catch {
      // An older agent, or a dropped call: the plugins' values are enough.
    }
    return { actions, events, variables };
  }

  /** The whole catalogue, with each plugin's events and settings form. */
  async pluginCatalogue(desktopId: string): Promise<{ plugins: PluginListing[] }> {
    return this.relay(desktopId, "GET", "/api/plugins");
  }

  installedPlugins(desktopId: string): Promise<{ plugins: InstalledPlugin[] }> {
    return this.relay(desktopId, "GET", "/api/plugins/installed");
  }

  /**
   * Install from one of the desktop's repositories. The desktop fetches the
   * package itself and checks it against the checksum that repository's index
   * pins; `accepted` must cover every permission it asks for, or the answer is
   * a `409` saying which.
   */
  async installPlugin(desktopId: string, repo: string, id: string, accepted: string[]): Promise<InstallAnswer> {
    try {
      return await this.relay<InstallAnswer>(desktopId, "POST", "/api/plugins/install", {
        source: { repository: { repo, id } },
        acceptedPermissions: accepted,
      });
    } catch (error) {
      if (error instanceof ConsentError) return error.answer;
      throw error;
    }
  }

  /**
   * What `action`'s arguments can be set to right now, from its plugin — OBS's
   * scenes, its inputs. `args` is what is filled in so far: the sources offered
   * depend on the scene chosen.
   */
  argumentChoices(
    desktopId: string,
    plugin: string,
    action: string,
    args: Record<string, string>,
  ): Promise<ArgumentChoices> {
    return this.relay(desktopId, "POST", "/api/plugins/choices", { plugin, action, args });
  }

  /** The largest package file that can be sent to this desktop the way it is reached. */
  uploadLimit(desktopId: string): number {
    return this.isRemote(desktopId) ? MAX_RELAYED_PACKAGE_BYTES : MAX_PACKAGE_BYTES;
  }

  /** What a package file is, before anything is installed. `bytes` is base64. */
  inspectPackage(desktopId: string, bytes: string): Promise<InspectedPackage> {
    return this.relay(desktopId, "POST", "/api/plugins/inspect", { source: { upload: bytes } });
  }

  /**
   * Install a package file. The desktop checks it exactly as it checks one from
   * a repository, except that there is no index to pin its checksum against.
   */
  async installPackage(desktopId: string, bytes: string, accepted: string[]): Promise<InstallAnswer> {
    try {
      return await this.relay<InstallAnswer>(desktopId, "POST", "/api/plugins/install", {
        source: { upload: bytes },
        acceptedPermissions: accepted,
      });
    } catch (error) {
      if (error instanceof ConsentError) return error.answer;
      throw error;
    }
  }

  uninstallPlugin(desktopId: string, id: string): Promise<unknown> {
    return this.relay(desktopId, "POST", "/api/plugins/uninstall", { id });
  }

  enablePlugin(desktopId: string, id: string, enabled: boolean): Promise<unknown> {
    return this.relay(desktopId, "POST", "/api/plugins/enable", { id, enabled });
  }

  reloadPlugin(desktopId: string, id: string): Promise<unknown> {
    return this.relay(desktopId, "POST", "/api/plugins/reload", { id });
  }

  /** Save settings. A secret left out keeps what the desktop has stored. */
  savePluginSettings(desktopId: string, id: string, values: Record<string, string>): Promise<unknown> {
    return this.relay(desktopId, "PUT", "/api/plugins/settings", { id, values });
  }

  pluginLog(desktopId: string, id: string): Promise<{ lines: string[] }> {
    return this.relay(desktopId, "POST", "/api/plugins/log", { id });
  }

  // -- the marketplace: the desktop's repositories, read by the desktop --------

  /** Every plugin the desktop's enabled repositories list, and the repositories. */
  catalog(desktopId: string): Promise<Catalog> {
    return this.relay(desktopId, "GET", "/api/plugins/catalog");
  }

  /** Read every repository again now, whatever its age. */
  refreshRepos(desktopId: string): Promise<{ repositories: Repository[] }> {
    return this.relay(desktopId, "POST", "/api/plugins/repositories/refresh", {});
  }

  /** What adding this address would add, before anything is saved. */
  previewRepo(desktopId: string, url: string): Promise<RepoPreview> {
    return this.relay(desktopId, "POST", "/api/plugins/repositories/preview", { url });
  }

  addRepo(desktopId: string, url: string): Promise<{ repository: Repository }> {
    return this.relay(desktopId, "POST", "/api/plugins/repositories/add", { url });
  }

  removeRepo(desktopId: string, url: string): Promise<unknown> {
    return this.relay(desktopId, "POST", "/api/plugins/repositories/remove", { url });
  }

  /** Turn a community repository on or off. */
  toggleRepo(desktopId: string, url: string, enabled: boolean): Promise<unknown> {
    return this.relay(desktopId, "POST", "/api/plugins/repositories/toggle", { url, enabled });
  }

  /** What each widget would show now, by id, from the agent's own templates. */
  async facePreviews(desktopId: string, widgets: WireWidget[]): Promise<Record<string, WidgetPreview>> {
    if (widgets.length === 0) return {};
    const answer = await this.relay<{ previews?: Record<string, WidgetPreview> }>(desktopId, "POST", "/api/faces/preview", { widgets });
    return answer.previews ?? {};
  }

  /** What one widget would show in look `look` (empty: whichever applies now). */
  facePreview(desktopId: string, widget: WireWidget, look = ""): Promise<WidgetPreview> {
    return this.relay(desktopId, "POST", "/api/faces/preview", { widget, look });
  }

  /** Keep a picture on the desktop (bytes as base64). Answers its `img:` name. */
  uploadPicture(desktopId: string, data: string): Promise<{ image: string }> {
    return this.relay(desktopId, "POST", "/api/images", { data });
  }

  pictures(desktopId: string): Promise<{ pictures?: { sha256: string; width: number; height: number }[] }> {
    return this.relay(desktopId, "GET", "/api/images");
  }

  removePicture(desktopId: string, sha: string): Promise<unknown> {
    return this.relay(desktopId, "DELETE", `/api/images/${sha}`);
  }

  iconPacks(desktopId: string): Promise<{ packs?: IconPackSummary[] }> {
    return this.relay(desktopId, "GET", "/api/iconpacks");
  }

  /** One pack's icons, fetched once per desktop and pack. */
  iconPack(desktopId: string, id: string): Promise<IconPack | null> {
    const key = `${desktopId}/${id}`;
    let pending = this.#packs.get(key);
    if (!pending) {
      pending = this.relay<IconPack>(desktopId, "GET", `/api/iconpacks/${encodeURIComponent(id)}`).catch(() => {
        this.#packs.delete(key);
        return null;
      });
      this.#packs.set(key, pending);
    }
    return pending;
  }

  /** Install a zip of icons (bytes as base64). */
  installIconPack(desktopId: string, data: string): Promise<{ pack: IconPackSummary; skipped: string[] }> {
    return this.relay(desktopId, "POST", "/api/iconpacks", { data });
  }

  removeIconPack(desktopId: string, id: string): Promise<unknown> {
    this.#packs.delete(`${desktopId}/${id}`);
    return this.relay(desktopId, "DELETE", `/api/iconpacks/${encodeURIComponent(id)}`);
  }

  locale(desktopId: string): Promise<DesktopLocale> {
    return this.relay(desktopId, "GET", "/api/locale");
  }

  saveLocale(desktopId: string, locale: DesktopLocale): Promise<DesktopLocale> {
    return this.relay(desktopId, "PUT", "/api/locale", locale);
  }

  /** `secrets` names the kept secrets as `source/widget/arg` (no widget: the source's own) — never their values. */
  sources(
    desktopId: string,
  ): Promise<{ sources?: DataSource[]; recipes?: Record<string, DataSource[]>; status?: Record<string, SourceStatus>; secrets?: string[] }> {
    return this.relay(desktopId, "GET", "/api/sources");
  }

  /** Forget a kept secret: a key's own, or (no widget) the source's. */
  forgetSourceSecret(desktopId: string, source: string, widget: string | null, arg: string): Promise<{ forgot: boolean }> {
    return this.relay(desktopId, "POST", "/api/sources/secrets/forget", { source, widget, arg });
  }

  saveSources(desktopId: string, sources: DataSource[]): Promise<unknown> {
    return this.relay(desktopId, "PUT", "/api/sources", { sources });
  }

  /** `widget`: the key it is tested for, whose kept secrets fill any left blank. */
  testSource(desktopId: string, source: DataSource, args: Record<string, string> = {}, widget?: string): Promise<SourceTest> {
    return this.relay(desktopId, "POST", "/api/sources/test", { source, args, widget });
  }

  config(desktopId: string): Promise<WireConfig> {
    return this.relay(desktopId, "GET", "/api/config");
  }

  beginPairing(desktopId: string): Promise<PairingBegin> {
    return this.relay(desktopId, "POST", "/api/pairing/begin", {});
  }

  cancelPairing(desktopId: string): Promise<unknown> {
    return this.relay(desktopId, "POST", "/api/pairing/cancel", {});
  }

  /// Replace the whole configuration. The agent records a revision, which is
  /// what makes undo work without the portal keeping a stack of its own.
  putConfig(desktopId: string, config: WireConfig): Promise<{ rev: number }> {
    return this.relay(desktopId, "PUT", "/api/config", config);
  }

  /// The page the tablets are showing, or `null` before any is known.
  page(desktopId: string): Promise<{ pageId: string | null }> {
    return this.relay(desktopId, "GET", "/api/page");
  }

  /// Turn every tablet to `pageId`.
  showPage(desktopId: string, pageId: string): Promise<{ pageId: string }> {
    return this.relay(desktopId, "PUT", "/api/page", { pageId });
  }

  undo(desktopId: string): Promise<{ rev: number }> {
    return this.relay(desktopId, "POST", "/api/undo", {});
  }

  redo(desktopId: string): Promise<{ rev: number }> {
    return this.relay(desktopId, "POST", "/api/redo", {});
  }

  revokeDevice(desktopId: string, deviceId: string): Promise<{ revoked: boolean }> {
    return this.relay(desktopId, "POST", "/api/devices/revoke", { deviceId });
  }

  // -- reactive layer -----------------------------------------------------

  /**
   * The full automations document. Returns a stub (`schema: 1, automations: []`)
   * on first run, which is what the editor treats as "no automations yet".
   */
  automations(desktopId: string): Promise<AutomationDocument> {
    return this.relay<AutomationDocument>(desktopId, "GET", "/api/automations");
  }

  /**
   * Replace the automations document. The agent stamps its own `rev` rather
   * than trusting the client's, so the body's `rev` is informational only —
   * the response carries the server's number back.
   */
  putAutomations(
    desktopId: string,
    document: AutomationDocument,
  ): Promise<{ rev: number }> {
    return this.relay<{ rev: number }>(
      desktopId,
      "PUT",
      "/api/automations",
      document,
    );
  }

  /**
   * Dry-run an automation against a scratch variable store. Admin-only on
   * the server; the portal passes through the session cookie and the relay
   * forwards the body unchanged.
   */
  dryRun(
    desktopId: string,
    automationId: string,
  ): Promise<Run> {
    return this.relay<Run>(desktopId, "POST", "/api/automations/dry-run", {
      automationId,
    });
  }
}

/** An install that needs permissions agreed to first. */
class ConsentError extends Error {
  readonly answer: InstallAnswer;

  constructor(answer: InstallAnswer) {
    super("this plugin needs permissions agreed to first");
    this.answer = answer;
  }
}

function isConsent(parsed: unknown): parsed is InstallAnswer {
  return typeof parsed === "object" && parsed !== null && (parsed as { needsConsent?: unknown }).needsConsent === true;
}

function extractError(parsed: unknown): string | null {
  if (typeof parsed !== "object" || parsed === null) {
    return null;
  }
  const candidate = (parsed as { error?: unknown }).error;
  return typeof candidate === "string" ? candidate : null;
}
