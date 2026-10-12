/**
 * The bridge to the agent running inside this process.
 *
 * The window does not call the agent's HTTP API. It could — the API is on
 * loopback — but that would mean putting the self-signed certificate and a
 * bearer token into a webview, and the window is the machine's own interface, so
 * it can simply ask. Only this file knows that, which is what keeps the rest of
 * the UI testable and the transport swappable.
 */
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  BrowserPairing,
  Catalog,
  InstallOutcome,
  PackageInfo,
  PairingPin,
  RepoPreview,
  Repository,
  Status,
  WindowPrefs,
} from "../../modules/shell/lib/types.ts";
import { previewShell } from "./preview.ts";

/** The event the shell emits when anything the window shows has changed. */
const STATE_EVENT = "shell://state";

const tauriShell = {
  status: () => invoke<Status>("status"),

  beginPairing: () => invoke<PairingPin>("begin_pairing"),

  /** Show a code for connecting a browser on another device. */
  beginBrowserPairing: () => invoke<BrowserPairing>("begin_browser_pairing"),

  /** Withdraw the code being shown, so it stops working at once. */
  cancelPairing: () => invoke<void>("cancel_pairing"),
  cancelBrowserPairing: () => invoke<void>("cancel_browser_pairing"),

  /** Allow or refuse what a browser elsewhere asked to do. */
  answerApproval: (id: number, allow: boolean) => invoke<void>("answer_approval", { id, allow }),

  setWebAccess: (enabled: boolean) => invoke<Status>("set_web_access", { enabled }),

  /**
   * Point this computer at a portal, or clear it with a blank address.
   *
   * The agent validates and persists it, then restarts the tunnel, so a corrected
   * address takes effect immediately instead of at the next restart.
   */
  setPortal: (url: string) => invoke<Status>("set_portal", { url }),

  revokeDevice: (deviceId: string) => invoke<void>("revoke_device", { deviceId }),

  /** Remove every paired tablet and browser. */
  revokeAllDevices: () => invoke<void>("revoke_all_devices"),

  /** How the window starts and closes. */
  windowPrefs: () => invoke<WindowPrefs>("window_prefs"),

  /** Change any of them; whatever is left out stays as it was. */
  setWindowPrefs: (patch: Partial<WindowPrefs>) => invoke<WindowPrefs>("set_window_prefs", patch),

  /** Open a web page in this computer's browser: the window has none of its own. */
  openLink: (url: string) => invoke<void>("open_link", { url }),

  /** What a package file is, before anything is installed. `bytes` is base64. */
  inspectPlugin: (bytes: string) =>
    invoke<{ package: PackageInfo; sha256: string; size: number; installedVersion: string | null }>("plugin_inspect", {
      bytes,
    }),

  installPluginFile: (bytes: string, accepted: string[]) =>
    invoke<InstallOutcome>("plugin_install_file", { bytes, accepted }),

  /** Install from one of this computer's repositories; its index pins the checksum. */
  installPluginFromRepo: (repo: string, id: string, accepted: string[]) =>
    invoke<InstallOutcome>("plugin_install_repo", { repo, id, accepted }),

  /** Every plugin the enabled repositories list, and the repositories. */
  catalog: (query = "") => invoke<Catalog>("plugin_catalog", { query }),

  /** Fetch every repository now, whatever its age. */
  refreshRepos: () => invoke<{ repositories: Repository[] }>("repos_refresh"),

  /** What adding this address would add, before anything is saved. */
  previewRepo: (url: string) => invoke<RepoPreview>("repo_preview", { url }),

  addRepo: (url: string) => invoke<Repository>("repo_add", { url }),

  removeRepo: (url: string) => invoke<void>("repo_remove", { url }),

  /** Turn a vetted repository on or off. */
  toggleRepo: (url: string, enabled: boolean) => invoke<void>("repo_toggle", { url, enabled }),

  uninstallPlugin: (id: string) => invoke<void>("plugin_uninstall", { id }),

  enablePlugin: (id: string, enabled: boolean) => invoke<void>("plugin_enable", { id, enabled }),

  reloadPlugin: (id: string) => invoke<void>("plugin_reload", { id }),

  /** Run a plugin from a folder, reloading it on every save. */
  linkPlugin: (path: string) => invoke<void>("plugin_link", { path }),

  savePluginSettings: (id: string, values: Record<string, string>) =>
    invoke<void>("plugin_settings", { id, values }),

  pluginLog: (id: string) => invoke<string[]>("plugin_log", { id }),

  /**
   * Watch for changes. Returns the unsubscribe function, which the caller must
   * keep and call: a listener that outlives its component keeps the window awake
   * for nothing.
   */
  onState: (handler: () => void): Promise<UnlistenFn> =>
    listen(STATE_EVENT, () => handler()),
};

export type Shell = typeof tauriShell;

/**
 * In a plain browser during development there is no agent to ask, so the window
 * runs against a scripted stand-in instead — enough to design and review every
 * screen without starting Tauri. `import.meta.env.DEV` is a build-time constant,
 * so a production bundle contains only the real bridge.
 */
export const shell: Shell = import.meta.env.DEV && !isTauri() ? previewShell() : tauriShell;
