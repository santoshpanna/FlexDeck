/**
 * A scripted agent for working on the window in a plain browser.
 *
 * Development only (see `client.ts`): it never ships. The state is plausible
 * rather than random, and it moves the way the real agent does — a code you
 * open is "used" by a tablet a few seconds later, an install lands in the
 * installed list — so every screen and transition can be seen without Tauri.
 *
 * `?scene=fresh` starts with nothing paired; `?scene=offline` with a tablet
 * that is away; `?approval` queues a request from a remote browser.
 */
import type {
  CatalogEntry,
  DeviceRow,
  InstallOutcome,
  InstalledPlugin,
  PackageInfo,
  Repository,
  Status,
  WindowPrefs,
} from "../../modules/shell/lib/types.ts";
import type { Shell } from "./client.ts";

const now = () => Math.floor(Date.now() / 1000);
const wait = (ms = 220) => new Promise<void>((resolve) => setTimeout(resolve, ms));

function installed(id: string, name: string, patch: Partial<InstalledPlugin> = {}): InstalledPlugin {
  return {
    id,
    name,
    version: "1.2.0",
    sha256: "",
    permissions: ["network"],
    enabled: true,
    source: { kind: "repository", name: "FlexDeck official" },
    installedAt: now() - 86400 * 12,
    running: true,
    status: null,
    error: null,
    actions: 9,
    ...patch,
  };
}

function entry(id: string, name: string, patch: Partial<CatalogEntry> = {}): CatalogEntry {
  return {
    id,
    name,
    version: "1.0.0",
    description: `${name} control from your deck: actions, live states and events.`,
    author: "FlexDeck",
    abi: 1,
    permissions: ["network"],
    latestUpdateDate: new Date(Date.now() - 86400_000 * 9).toISOString(),
    package: { url: "", sha256: "", size: 182_000 },
    repo: { url: "https://plugins.flexdeck.dev", name: "FlexDeck official", kind: "official" },
    iconData: null,
    installedVersion: null,
    installedFromHere: false,
    update: false,
    ...patch,
  };
}

export function previewShell(): Shell {
  const params = new URLSearchParams(location.search);
  const scene = params.get("scene") ?? "live";
  let windowPrefs: WindowPrefs = { startMinimized: false, closeToTray: true, minimizeToTray: false, autostart: false };
  const listeners = new Set<() => void>();
  const emit = () => listeners.forEach((listener) => listener());

  let devices: DeviceRow[] =
    scene === "fresh"
      ? []
      : [
          { id: "t1", name: "Studio Tab", created_at: now() - 86400 * 20, online: scene !== "offline", kind: "tablet" },
          { id: "b1", name: "Chrome on this PC", created_at: now() - 86400 * 3, online: true, kind: "browser-local" },
        ];
  let plugins: InstalledPlugin[] = [
    installed("obs", "OBS Studio", { actions: 18, settings: [{ key: "password", description: "WebSocket password", kind: "text", choices: [], defaultValue: null, secret: true, value: null, isSet: true }] }),
    installed("spotify", "Spotify", { version: "1.1.2", status: "Not signed in — open Settings", source: { kind: "repository", name: "Community" } }),
    installed("hue", "Philips Hue", { enabled: false, running: false, actions: 12 }),
  ];
  let catalog: CatalogEntry[] = [
    entry("obs", "OBS Studio", { installedVersion: "1.2.0", installedFromHere: true, version: "1.3.0", update: true }),
    entry("discord", "Discord", { permissions: ["network", "notifications"] }),
    entry("voicemeeter", "Voicemeeter", { author: "vm-fans", repo: { url: "https://vm.example", name: "Community", kind: "vetted" } }),
    entry("twitch", "Twitch", { permissions: ["network"] }),
    entry("shell", "Shell commands", { author: "someone", permissions: ["processes"], repo: { url: "https://x.example", name: "x.example", kind: "custom" } }),
  ];
  let repositories: Repository[] = [
    { url: "https://plugins.flexdeck.dev", name: "FlexDeck official", description: "", website: null, kind: "official", enabled: true, pluginCount: 4, fetchedAt: now() - 3600, error: null, dropped: [] },
    { url: "https://vm.example", name: "Community", description: "", website: null, kind: "vetted", enabled: true, pluginCount: 1, fetchedAt: now() - 3600, error: null, dropped: [] },
    { url: "https://x.example", name: "x.example", description: "", website: null, kind: "custom", enabled: true, pluginCount: 1, fetchedAt: now() - 7200, error: null, dropped: [] },
  ];
  let webOn = scene !== "fresh";
  let pairing: { pin: string; expiresAt: number } | null = null;
  let browserPairing: { code: string; expiresAt: number } | null = null;
  let approvals = params.has("approval")
    ? [{ id: 1, action: "Install a plugin", detail: "Discord 1.0.0 from FlexDeck official", requester: "Pixel · Chrome" }]
    : [];

  const status = (): Status => ({
    fingerprint: "a3f921c0e8b74d1f",
    host: "192.168.1.24",
    port: 23539,
    warnings: [],
    server: { error: null },
    tunnel: { enabled: webOn, running: webOn, refused: null, lastError: null, portal: "wss://deck.example.net/tunnel" },
    devices,
    plugins: {
      builtIn: [
        { id: "system", plugin: "System audio", version: "", actions: 6, status: null },
        { id: "keys", plugin: "Keyboard", version: "", actions: 3, status: null },
      ],
      installed: plugins,
    },
    pairing,
    browserPairing,
    approvals,
  });

  const install = (id: string): InstallOutcome => {
    const found = catalog.find((candidate) => candidate.id === id);
    const plugin = installed(id, found?.name ?? id, { installedAt: now() });
    plugins = [plugin, ...plugins.filter((candidate) => candidate.id !== id)];
    catalog = catalog.map((candidate) =>
      candidate.id === id ? { ...candidate, installedVersion: candidate.version, installedFromHere: true, update: false } : candidate,
    );
    emit();
    return { installed: plugin };
  };

  const pkg: PackageInfo = { id: "local", name: "My plugin", version: "0.1.0", permissions: ["network"] };

  return {
    status: async () => (await wait(), status()),
    beginPairing: async () => {
      await wait();
      pairing = { pin: String(1000 + Math.floor(Math.random() * 9000)), expiresAt: now() + 300 };
      // A tablet "types" it a little later, which uses the code up — as the agent does.
      setTimeout(() => {
        pairing = null;
        devices = [...devices, { id: `t${Date.now()}`, name: "Kitchen Tab", created_at: now(), online: true, kind: "tablet" }];
        emit();
      }, 7000);
      return pairing;
    },
    beginBrowserPairing: async () => {
      await wait();
      browserPairing = { code: `${Math.random().toString(36).slice(2, 6)}-${Math.random().toString(36).slice(2, 6)}`.toUpperCase(), expiresAt: now() + 300 };
      emit();
      return browserPairing;
    },
    cancelPairing: async () => {
      await wait();
      pairing = null;
      emit();
    },
    cancelBrowserPairing: async () => {
      await wait();
      browserPairing = null;
      emit();
    },
    answerApproval: async (id) => {
      await wait();
      approvals = approvals.filter((request) => request.id !== id);
      emit();
    },
    setWebAccess: async (enabled) => {
      await wait(400);
      webOn = enabled;
      emit();
      return status();
    },
    setPortal: async () => (await wait(), status()),
    windowPrefs: async () => (await wait(), { ...windowPrefs }),
    setWindowPrefs: async (patch) => {
      await wait();
      windowPrefs = { ...windowPrefs, ...patch };
      return { ...windowPrefs };
    },
    openLink: async (url) => void window.open(url, "_blank", "noopener"),
    revokeAllDevices: async () => {
      await wait();
      devices = [];
      emit();
    },
    revokeDevice: async (deviceId) => {
      await wait();
      devices = devices.filter((device) => device.id !== deviceId);
      emit();
    },
    inspectPlugin: async () => (await wait(), { package: pkg, sha256: "", size: 1000, installedVersion: null }),
    installPluginFile: async () => (await wait(600), install("local")),
    installPluginFromRepo: async (_repo, id) => (await wait(600), install(id)),
    catalog: async () => (await wait(), { plugins: catalog, repositories, vettedError: null }),
    refreshRepos: async () => (await wait(500), { repositories }),
    previewRepo: async (url) => (await wait(400), { url, name: url, description: "A repository someone runs.", website: null, pluginCount: 2, plugins: ["Thing", "Other thing"], kind: "custom" }),
    addRepo: async (url) => {
      await wait();
      const repo: Repository = { url, name: url, description: "", website: null, kind: "custom", enabled: true, pluginCount: 2, fetchedAt: now(), error: null, dropped: [] };
      repositories = [...repositories, repo];
      return repo;
    },
    removeRepo: async (url) => {
      await wait();
      repositories = repositories.filter((repo) => repo.url !== url);
    },
    toggleRepo: async (url, enabled) => {
      await wait();
      repositories = repositories.map((repo) => (repo.url === url ? { ...repo, enabled } : repo));
    },
    uninstallPlugin: async (id) => {
      await wait();
      plugins = plugins.filter((plugin) => plugin.id !== id);
      catalog = catalog.map((candidate) => (candidate.id === id ? { ...candidate, installedVersion: null, installedFromHere: false } : candidate));
      emit();
    },
    enablePlugin: async (id, enabled) => {
      await wait();
      plugins = plugins.map((plugin) => (plugin.id === id ? { ...plugin, enabled, running: enabled } : plugin));
      emit();
    },
    reloadPlugin: async () => wait(),
    linkPlugin: async () => wait(),
    savePluginSettings: async () => wait(),
    pluginLog: async (id) => (await wait(), [`[${id}] started`, `[${id}] connected`, `[${id}] ready`]),
    onState: async (handler) => {
      listeners.add(handler);
      return () => listeners.delete(handler);
    },
  };
}
