/**
 * The window's vocabulary.
 *
 * Types only, and in their own module, so the pure functions under `lib/` can be
 * tested with `node --test` without pulling in the Tauri runtime — a `import
 * type` is erased, a runtime import is not.
 */

/** Why the portal refused this desktop, if it did. */
export interface Refusal {
  code: string;
  message: string;
}

export interface TunnelState {
  enabled: boolean;
  /** Connected right now — NOT "the reconnect loop is alive". */
  running: boolean;
  /**
   * The portal's stated refusal, or `null`. Distinct from `running === false`:
   * an unreachable portal is a network problem, while a refusal means the portal
   * answered and said no — usually a version mismatch.
   */
  refused: Refusal | null;
  /**
   * Why the last connection attempt failed, in the transport's own words, or
   * `null` once one gets through. The difference between "could not be reached"
   * and a line that names the actual problem (a wrong scheme, a wrong port).
   */
  lastError: string | null;
  portal: string | null;
}

/** What a paired credential is. */
export type DeviceKind = "tablet" | "browser-local" | "browser-remote" | "agent";

export interface DeviceRow {
  id: string;
  name: string;
  /** Unix seconds. Snake_case because the agent's own API spells it this way. */
  created_at: number;
  online: boolean;
  kind: DeviceKind;
}

/** The code for connecting a browser on another device. */
export interface BrowserPairing {
  /** Shown as `XXXX-XXXX`. */
  code: string;
  expiresAt: number;
}

/** Something a browser elsewhere asked to do, waiting for a yes here. */
export interface ApprovalRequest {
  id: number;
  action: string;
  detail: string;
  /** The paired browser's name. */
  requester: string;
}

/** A plugin compiled into the agent. */
export interface BuiltInPlugin {
  id: string;
  plugin: string;
  version: string;
  actions: number;
  status: string | null;
}

/** One field of a plugin's settings form, as the agent describes it. */
export interface PluginSetting {
  key: string;
  description: string;
  kind: "text" | "number" | "bool" | "choice";
  choices: string[];
  defaultValue: string | null;
  /** Never sent back once saved; `isSet` says whether there is one. */
  secret: boolean;
  /** Kept by the plugin (a sign-in): not in the form, cleared by signing out. */
  internal?: boolean;
  help?: string | null;
  value: string | null;
  isSet: boolean;
}

/** A plugin the person installed. */
export interface InstalledPlugin {
  id: string;
  name: string;
  version: string;
  sha256: string;
  permissions: string[];
  enabled: boolean;
  source: { kind: "file" | "url" | "repository" | "marketplace" | "dev"; url?: string; path?: string; name?: string };
  installedAt: number;
  running: boolean;
  /** Why a running plugin is unwell, in its host's words. */
  status: string | null;
  /** Why it could not be started at all. */
  error: string | null;
  actions?: number;
  description?: string;
  settings?: PluginSetting[];
  /** Where its setup is explained. */
  guide?: string | null;
}

/** What a package says it is, before it is installed. */
export interface PackageInfo {
  id: string;
  name: string;
  version: string;
  description?: string;
  author?: string;
  homepage?: string | null;
  permissions?: string[];
}

/** Which list a repository is on. */
export type RepoKind = "official" | "vetted" | "custom";

/** One plugin in the marketplace: a repository's entry, as the agent merged it. */
export interface CatalogEntry {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  homepage?: string;
  abi: number;
  permissions: string[];
  /** When the plugin first appeared (RFC 3339). */
  releaseDate?: string;
  /** When this version was published (RFC 3339). */
  latestUpdateDate?: string;
  package: { url: string; sha256: string; size: number };
  repo: { url: string; name: string; kind: RepoKind };
  /** The icon as a `data:` URI; the agent fetched it, so the window never does. */
  iconData: string | null;
  installedVersion: string | null;
  /** Whether the installed copy came from this same repository. */
  installedFromHere: boolean;
  update: boolean;
}

/** A repository the marketplace reads. */
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

/** An install waiting for the person to agree to its permissions. */
export interface Consent {
  source: { kind: "file"; bytes: string } | { kind: "repository"; repo: string; id: string };
  package: PackageInfo;
  permissions: string[];
  /** Set when this would replace an installed version. */
  replaces: string | null;
  /** Where it comes from, when that is a repository. */
  repo?: { name: string; kind: RepoKind };
  icon?: string | null;
}

/** What an install answered: done, or "ask first". */
export type InstallOutcome =
  | { installed: InstalledPlugin }
  | { needsConsent: true; missing: string[]; package: PackageInfo; sha256: string };

export interface PairingPin {
  pin: string;
  expiresAt: number;
}

export interface ServerState {
  /** Why nothing outside this machine can reach it, or `null` if all is well. */
  error: string | null;
}

export interface McpState {
  url: string;
  error: string | null;
}

export interface AgentToken {
  token: string;
  deviceId: string;
  name: string;
  url: string;
}

export interface Status {
  fingerprint: string;
  host: string;
  port: number;
  warnings: string[];
  server: ServerState;
  mcp: McpState;
  tunnel: TunnelState;
  devices: DeviceRow[];
  plugins: {
    builtIn: BuiltInPlugin[];
    installed: InstalledPlugin[];
  };
  pairing: PairingPin | null;
  browserPairing: BrowserPairing | null;
  approvals: ApprovalRequest[];
}

/** How the window starts, closes and minimizes. `autostart` is read from the OS each time. */
export interface WindowPrefs {
  startMinimized: boolean;
  closeToTray: boolean;
  minimizeToTray: boolean;
  autostart: boolean;
  logLevel: LogLevel;
  logLevelOverridden: boolean;
}

export type LogLevel = "error" | "warn" | "info" | "debug" | "trace";
