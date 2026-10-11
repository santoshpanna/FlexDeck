import { Children, cloneElement, useCallback, useEffect, useMemo, useState, type ReactElement } from "react";

import { shell } from "../../../services/tauri/client.ts";
import { nextStep, readouts } from "../lib/home.lib.ts";
import { pluginGroups, type PluginGroup } from "../lib/plugins.lib.ts";
import { statusRows, type StatusRow } from "../lib/status.lib.ts";
import type {
  Catalog,
  CatalogEntry,
  Consent,
  InstallOutcome,
  PairingPin,
  RepoPreview,
  Status,
  TunnelState,
} from "../lib/types.ts";
import type { ShellViewProps } from "../view/shell.view.tsx";

export interface ShellControllerProps {
  children: ReactElement;
}

/**
 * Owns everything the window does: reading state, acting on it, and phrasing
 * failures. The view renders; it does not decide.
 */
export function ShellController({ children }: ShellControllerProps) {
  const [status, setStatus] = useState<Status | null>(null);
  const [pairing, setPairing] = useState<PairingPin | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [consent, setConsent] = useState<Consent | null>(null);
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [preview, setPreview] = useState<RepoPreview | null>(null);
  const [logs, setLogs] = useState<Record<string, string[]>>({});

  const load = useCallback(async () => {
    try {
      setStatus(await shell.status());
      setError(null);
    } catch (caught) {
      // One error slot, not one per card: two would let the window disagree with
      // itself about what went wrong.
      setError(describe(caught));
    }
  }, []);

  /**
   * Read the marketplace. Not on every state event: the agent fetches the
   * repositories at most every few hours, but the icons and the merge are
   * still work, and nothing a tablet does changes what is listed.
   */
  const loadCatalog = useCallback(async () => {
    try {
      setCatalog(await shell.catalog());
    } catch (caught) {
      setError(describe(caught));
    }
  }, []);

  useEffect(() => {
    void loadCatalog();
  }, [loadCatalog]);

  useEffect(() => {
    void load();

    // The agent emits on every change it can see — a device connecting, a
    // configuration edit, the tunnel coming up. Re-reading on that signal is why
    // presence in the window is live rather than a snapshot from page load.
    let stop: (() => void) | null = null;
    void shell.onState(() => void load()).then((unlisten) => {
      stop = unlisten;
    });

    return () => stop?.();
  }, [load]);

  const act = useCallback(
    async (run: () => Promise<unknown>) => {
      setBusy(true);
      try {
        await run();
        await load();
      } catch (caught) {
        setError(describe(caught));
      } finally {
        setBusy(false);
      }
    },
    [load],
  );

  const rows: StatusRow[] = useMemo(() => (status ? statusRows(status) : []), [status]);
  const plugins: PluginGroup[] = useMemo(
    () => (status ? pluginGroups(status.plugins.builtIn) : []),
    [status],
  );

  // A placeholder until the first read lands, matching the view's own default.
  // "No portal" rather than "off", so the toggle cannot appear for an
  // installation that has no portal and then vanish.
  const tunnel: TunnelState = status?.tunnel ?? {
    enabled: false,
    running: false,
    refused: null,
    lastError: null,
    portal: null,
  };

  const installed = status?.plugins.installed ?? [];

  /** Install what the person agreed to, or ask again if the agent wants more. */
  const install = async (pending: Consent) => {
    const outcome: InstallOutcome =
      pending.source.kind === "file"
        ? await shell.installPluginFile(pending.source.bytes, pending.permissions)
        : await shell.installPluginFromRepo(pending.source.repo, pending.source.id, pending.permissions);
    if ("needsConsent" in outcome) {
      // The package asks for more than the listing said; ask again, with the
      // package's own list, rather than installing on the old answer.
      setConsent({ ...pending, package: outcome.package, permissions: outcome.package.permissions ?? outcome.missing });
      return;
    }
    setConsent(null);
    await loadCatalog();
  };

  const fetchedAt = useMemo(() => {
    const times = (catalog?.repositories ?? []).flatMap((repo) => (repo.fetchedAt ? [repo.fetchedAt] : []));
    return times.length > 0 ? Math.min(...times) : null;
  }, [catalog]);

  const props: ShellViewProps = {
    rows,
    readouts: status ? readouts(status) : [],
    next: status ? nextStep(status) : null,
    address: status ? status.host : "",
    port: status ? status.port : null,
    devices: status?.devices ?? [],
    plugins,
    installed,
    consent,
    catalog: catalog?.plugins ?? null,
    repositories: catalog?.repositories ?? [],
    vettedError: catalog?.vettedError ?? null,
    fetchedAt,
    preview,
    logs,
    // The PIN this window opened, or else one the agent already has open — so a
    // window reopened mid-pairing still shows the code being typed.
    pairing: pairing ?? status?.pairing ?? null,
    browserPairing: status?.browserPairing ?? null,
    approvals: status?.approvals ?? [],
    fingerprint: status?.fingerprint ?? "",
    tunnel,
    error,
    busy,
    loading: status === null && error === null,
    onNewPin: () =>
      void act(async () => {
        setPairing(await shell.beginPairing());
      }),
    onCancelPin: () =>
      void act(async () => {
        await shell.cancelPairing();
        setPairing(null);
      }),
    onNewBrowserCode: () => void act(() => shell.beginBrowserPairing()),
    onCancelBrowserCode: () => void act(() => shell.cancelBrowserPairing()),
    onAnswer: (id: number, allow: boolean) => void act(() => shell.answerApproval(id, allow)),
    onRevoke: (deviceId: string) => void act(() => shell.revokeDevice(deviceId)),
    onToggleWebAccess: (enabled: boolean) => void act(() => shell.setWebAccess(enabled)),
    onSetPortal: (url: string) => void act(() => shell.setPortal(url)),
    onOpenLink: (url: string) => void act(() => shell.openLink(url)),
    onChooseFile: (file: File) =>
      void act(async () => {
        const bytes = await base64(file);
        const inspected = await shell.inspectPlugin(bytes);
        setConsent({
          source: { kind: "file", bytes },
          package: inspected.package,
          permissions: inspected.package.permissions ?? [],
          replaces: inspected.installedVersion,
        });
      }),
    onPick: (entry: CatalogEntry) => {
      setConsent({
        source: { kind: "repository", repo: entry.repo.url, id: entry.id },
        package: {
          id: entry.id,
          name: entry.name,
          version: entry.version,
          description: entry.description,
          author: entry.author,
          homepage: entry.homepage ?? null,
        },
        permissions: entry.permissions,
        replaces: entry.installedVersion,
        repo: { name: entry.repo.name, kind: entry.repo.kind },
        icon: entry.iconData,
      });
    },
    onRefresh: () =>
      void act(async () => {
        await shell.refreshRepos();
        await loadCatalog();
      }),
    onPreviewRepo: (url: string) =>
      void act(async () => {
        setPreview(await shell.previewRepo(url));
      }),
    onAddRepo: (url: string) =>
      void act(async () => {
        await shell.addRepo(url);
        setPreview(null);
        await loadCatalog();
      }),
    onCancelPreview: () => setPreview(null),
    onRemoveRepo: (url: string) =>
      void act(async () => {
        await shell.removeRepo(url);
        await loadCatalog();
      }),
    onToggleRepo: (url: string, enabled: boolean) =>
      void act(async () => {
        await shell.toggleRepo(url, enabled);
        await loadCatalog();
      }),
    onAccept: () => {
      if (consent) void act(() => install(consent));
    },
    onCancel: () => setConsent(null),
    onLink: (path: string) => void act(() => shell.linkPlugin(path)),
    onUninstall: (id: string) =>
      void act(async () => {
        await shell.uninstallPlugin(id);
        await loadCatalog();
      }),
    onEnable: (id: string, enabled: boolean) => void act(() => shell.enablePlugin(id, enabled)),
    onReload: (id: string) => void act(() => shell.reloadPlugin(id)),
    onSaveSettings: (id: string, values: Record<string, string>) =>
      void act(() => shell.savePluginSettings(id, values)),
    onShowLog: (id: string) =>
      void shell.pluginLog(id).then(
        (lines) => setLogs((current) => ({ ...current, [id]: lines })),
        (caught) => setError(describe(caught)),
      ),
  };

  return cloneElement(Children.only(children), props);
}

/** A file's bytes as base64, which is how they cross into the agent. */
function base64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(",")[1] ?? "");
    reader.onerror = () => reject(new Error(`could not read ${file.name}`));
    reader.readAsDataURL(file);
  });
}

function describe(caught: unknown): string {
  if (caught instanceof Error) {
    return caught.message;
  }
  return String(caught);
}
