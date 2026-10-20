import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useRef, useState, type DragEvent, type ReactElement } from "react";

import { Glyph, HoldToConfirm, InfoTip, Key, Led, Notice, Segmented, Sheet, Switch, cn, item, keyVariants, list, spring } from "@flexdeck/ui";

import { Card } from "../../../../common/components/Card/Card.component.tsx";
import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import type {
  Catalog,
  CatalogEntry,
  InstalledPlugin,
  PluginListing,
  PluginSetting,
  RepoPreview,
  RepositoryIconPack,
} from "../../../../services/api/api.ts";
import {
  MARKET_TYPES,
  checkPackageFile,
  entryAction,
  filterCatalog,
  filterCounts,
  filterIconPacks,
  iconPackCounts,
  kindLabel,
  permissionLabel,
  pluginState,
  relativeDate,
  type MarketFilter,
  type MarketType,
} from "../../lib/plugins.lib.ts";
import { control } from "../Flow/fields.tsx";

export interface PluginsPanelProps {
  desktopId: string;
  /** Called after anything that changes what the desktop can do. */
  onChanged?: () => void;
}

/** An install waiting for a yes: from a repository, or from a dropped file. */
interface Pending {
  source: { kind: "repository"; repo: string; id: string } | { kind: "file"; bytes: string; fileName: string };
  name: string;
  version: string;
  author: string;
  description: string;
  iconData: string | null;
  /** Where it comes from, in words: a repository's name or a file's. */
  from: string;
  /** Why to think twice, when there is a reason. */
  unreviewed: "repository" | "file" | null;
  replaces: string | null;
  permissions: string[];
}

function pendingFromEntry(entry: CatalogEntry): Pending {
  return {
    source: { kind: "repository", repo: entry.repo.url, id: entry.id },
    name: entry.name,
    version: entry.version,
    author: entry.author,
    description: entry.description,
    iconData: entry.iconData,
    from: entry.repo.name,
    unreviewed: entry.repo.kind === "custom" ? "repository" : null,
    replaces: entry.installedVersion,
    permissions: entry.permissions,
  };
}

/** A file's bytes as base64, which is how a package travels to the desktop. */
function base64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(",")[1] ?? "");
    reader.onerror = () => reject(new Error(`${file.name} could not be read`));
    reader.readAsDataURL(file);
  });
}

/** Whether a drag carries files, as opposed to text or a link. */
function carriesFiles(event: DragEvent): boolean {
  return Array.from(event.dataTransfer.types).includes("Files");
}

const FILTERS: { filter: MarketFilter; label: string }[] = [
  { filter: "all", label: "All" },
  { filter: "official", label: "Official" },
  { filter: "vetted", label: "Community" },
  { filter: "custom", label: "Custom" },
  { filter: "updates", label: "Updates" },
];

/**
 * Plugins on this desktop, and the marketplace to get more from.
 *
 * The marketplace is the desktop's: it reads its own plugin repositories —
 * the official one, the community ones flexdeck reviewed, and any the person
 * added — and this page shows what it found. Installing goes through the
 * desktop too: it downloads the package itself and checks it against the
 * checksum the repository's index pins, so this page never handles the code
 * it installs.
 *
 * Nothing installs without the permissions being shown and agreed to; the
 * desktop refuses an install whose permissions were not all accepted, so this
 * page is the explanation, not the lock.
 */
export function PluginsPanel({ desktopId, onChanged }: PluginsPanelProps): ReactElement {
  const api = useApi();
  const [installed, setInstalled] = useState<InstalledPlugin[]>([]);
  const [catalogue, setCatalogue] = useState<PluginListing[]>([]);
  const [market, setMarket] = useState<Catalog | null>(null);
  const [packs, setPacks] = useState<RepositoryIconPack[]>([]);
  const [type, setType] = useState<MarketType>("integrations");
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<MarketFilter>("all");
  const [open, setOpen] = useState<string | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [preview, setPreview] = useState<RepoPreview | null>(null);
  const [address, setAddress] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  // dragenter and dragleave fire for every child crossed; count them, so the
  // highlight goes off only when the drag has really left the panel.
  const depth = useRef(0);
  const fileInput = useRef<HTMLInputElement>(null);

  const load = useCallback(async () => {
    const [mine, all] = await Promise.all([api.installedPlugins(desktopId), api.pluginCatalogue(desktopId)]);
    setInstalled(mine.plugins);
    setCatalogue(all.plugins);
  }, [api, desktopId]);

  const loadMarket = useCallback(async () => {
    const [catalog, iconPacks] = await Promise.all([
      api.catalog(desktopId),
      api.repositoryIconPacks(desktopId).catch(() => ({ packs: [] as RepositoryIconPack[] })),
    ]);
    setMarket(catalog);
    setPacks(iconPacks.packs ?? []);
  }, [api, desktopId]);

  useEffect(() => {
    setError(null);
    void load().catch((caught: unknown) => setError(String((caught as Error).message ?? caught)));
    void loadMarket().catch((caught: unknown) => {
      setMarket({ plugins: [], repositories: [], vettedError: null });
      setError(String((caught as Error).message ?? caught));
    });
  }, [load, loadMarket]);

  const act = async (work: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await work();
      await load();
      await loadMarket();
      onChanged?.();
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      setBusy(false);
    }
  };

  const install = (request: Pending) =>
    act(async () => {
      const answer =
        request.source.kind === "repository"
          ? await api.installPlugin(desktopId, request.source.repo, request.source.id, request.permissions)
          : await api.installPackage(desktopId, request.source.bytes, request.permissions);
      if ("needsConsent" in answer) {
        // The package asks for more than its listing said: ask again.
        setPending({ ...request, permissions: answer.package.permissions ?? answer.missing });
        return;
      }
      setPending(null);
    });

  /**
   * A package file, dropped or chosen: the desktop reads it and says what it
   * is, and that is what the consent card shows — not the file's name, which
   * anyone can choose.
   */
  const choose = (file: File) => {
    const problem = checkPackageFile(file.name, file.size, api.uploadLimit(desktopId), api.isRemote(desktopId));
    if (problem) {
      setError(problem);
      return;
    }
    void act(async () => {
      const bytes = await base64(file);
      const inspected = await api.inspectPackage(desktopId, bytes);
      setPending({
        source: { kind: "file", bytes, fileName: file.name },
        name: inspected.package.name,
        version: inspected.package.version,
        author: inspected.package.author ?? "",
        description: inspected.package.description ?? "",
        iconData: null,
        from: file.name,
        unreviewed: "file",
        replaces: inspected.installedVersion,
        permissions: inspected.package.permissions ?? [],
      });
    });
  };

  const drop = {
    onDragEnter: (event: DragEvent) => {
      if (!carriesFiles(event)) return;
      event.preventDefault();
      depth.current += 1;
      setDragging(true);
    },
    onDragOver: (event: DragEvent) => {
      if (!carriesFiles(event)) return;
      // Without this the browser opens the file instead of dropping it here.
      event.preventDefault();
      event.dataTransfer.dropEffect = busy ? "none" : "copy";
    },
    onDragLeave: (event: DragEvent) => {
      if (!carriesFiles(event)) return;
      depth.current = Math.max(0, depth.current - 1);
      if (depth.current === 0) setDragging(false);
    },
    onDrop: (event: DragEvent) => {
      if (!carriesFiles(event)) return;
      event.preventDefault();
      depth.current = 0;
      setDragging(false);
      const files = Array.from(event.dataTransfer.files);
      if (busy || files.length === 0) return;
      if (files.length > 1) {
        setError("Drop one package at a time.");
        return;
      }
      choose(files[0]!);
    },
  };

  const builtIn = catalogue.filter((plugin) => plugin.origin === "builtIn");
  const entries = market?.plugins ?? [];
  const counts = type === "iconPacks" ? iconPackCounts(packs) : filterCounts(entries);
  const shown = filterCatalog(entries, query, filter);
  const shownPacks = filterIconPacks(packs, query, filter);
  const repositories = market?.repositories ?? [];
  const times = repositories.flatMap((repo) => (repo.fetchedAt ? [repo.fetchedAt] : []));
  const read = relativeDate(times.length > 0 ? Math.min(...times) : null);

  return (
    <div className="relative grid items-start gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(0,1.3fr)]" {...drop}>
      <AnimatePresence>
        {dragging ? (
          <motion.div
            aria-hidden="true"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="pointer-events-none fixed inset-4 z-40 flex items-center justify-center rounded-[28px] border-2 border-dashed border-signal bg-signal-soft/70 backdrop-blur-sm"
          >
            <span className="font-mono text-sm tracking-[0.2em] text-signal uppercase">Drop the .fdpkg to install it</span>
          </motion.div>
        ) : null}
      </AnimatePresence>

      {error ? (
        <div className="xl:col-span-2">
          <Notice>{error}</Notice>
        </div>
      ) : null}

      <div className="flex flex-col gap-5">
        <Card title={`Installed · ${installed.length}`}>
          {installed.length === 0 ? (
            <p className="m-0 text-sm text-muted">None yet — the marketplace is beside this.</p>
          ) : (
            <motion.ul variants={list} initial="initial" animate="animate" className="m-0 flex list-none flex-col gap-2 p-0">
              {installed.map((plugin) => (
                <InstalledRow
                  key={plugin.id}
                  plugin={plugin}
                  settings={catalogue.find((entry) => entry.id === plugin.id)?.settings ?? []}
                  guide={catalogue.find((entry) => entry.id === plugin.id)?.guide ?? null}
                  busy={busy}
                  onEnable={(enabled) => void act(() => api.enablePlugin(desktopId, plugin.id, enabled))}
                  onReload={() => void act(() => api.reloadPlugin(desktopId, plugin.id))}
                  onRemove={() => void act(() => api.uninstallPlugin(desktopId, plugin.id))}
                  onSave={(values) => void act(() => api.savePluginSettings(desktopId, plugin.id, values))}
                  onSignOut={() => void act(() => api.signOutPlugin(desktopId, plugin.id))}
                  onLog={() => api.pluginLog(desktopId, plugin.id).then((answer) => answer.lines)}
                />
              ))}
            </motion.ul>
          )}
          {builtIn.length > 0 ? (
            <div className="flex flex-wrap gap-1.5">
              <span className="lbl mr-1 self-center">Built in</span>
              {builtIn.map((plugin) => (
                <span key={plugin.id} className="flex items-center gap-1.5 rounded-full border border-line bg-body px-2.5 py-1 text-[12px]">
                  <Led state="ok" size="sm" /> {plugin.name}
                </span>
              ))}
            </div>
          ) : null}
        </Card>

        <Card title="Install from a file">
          <button
            type="button"
            disabled={busy}
            onClick={() => fileInput.current?.click()}
            className="flex cursor-pointer flex-col items-center gap-1.5 rounded-[14px] border border-dashed border-line-strong bg-body px-4 py-6 text-center transition-colors hover:border-signal disabled:opacity-50"
          >
            <Glyph name="file" size={20} className="text-muted" />
            <span className="text-sm">Drop a plugin package here</span>
            <span className="text-[12px] text-muted">
              or click to choose a .fdpkg file
              {api.isRemote(desktopId) ? ` — up to ${Math.round(api.uploadLimit(desktopId) / 1024)} KB through the cloud` : ""}
            </span>
          </button>
          <input
            ref={fileInput}
            type="file"
            accept=".fdpkg"
            hidden
            onChange={(event) => {
              const file = event.currentTarget.files?.[0];
              if (file) choose(file);
              event.currentTarget.value = "";
            }}
          />
        </Card>

        <Card title={`Repositories · ${repositories.length}`}>
          <ul className="m-0 flex list-none flex-col gap-2 p-0">
            {repositories.map((repo) => (
              <li key={repo.url} className={cn("flex items-start gap-3 rounded-[14px] border border-line bg-body p-3", !repo.enabled && "opacity-60")}>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-sm">{repo.name}</span>
                    <span className={cn("rounded-md border px-1.5 py-0.5 font-mono text-[9px] tracking-[0.14em] uppercase", repo.kind === "custom" ? "border-warn/40 text-warn" : "border-line text-muted")}>
                      {kindLabel(repo.kind)}
                    </span>
                  </div>
                  <div className="font-mono text-[10px] text-muted">
                    {repo.pluginCount === 1 ? "1 plugin" : `${repo.pluginCount} plugins`}
                    {repo.fetchedAt ? ` · read ${relativeDate(repo.fetchedAt)}` : ""}
                    {repo.kind === "custom" ? ` · ${repo.url}` : ""}
                  </div>
                  {repo.error ? <p className="m-0 mt-1 text-[12px] text-warn">Could not refresh: {repo.error}</p> : null}
                </div>
                {repo.kind === "vetted" ? (
                  <Switch
                    size="sm"
                    checked={repo.enabled}
                    disabled={busy}
                    label={`${repo.enabled ? "Turn off" : "Turn on"} ${repo.name}`}
                    onChange={(enabled) => void act(() => api.toggleRepo(desktopId, repo.url, enabled))}
                  />
                ) : repo.kind === "custom" ? (
                  <HoldToConfirm size="sm" tone="danger" disabled={busy} onConfirm={() => void act(() => api.removeRepo(desktopId, repo.url))}>
                    Remove
                  </HoldToConfirm>
                ) : null}
              </li>
            ))}
          </ul>
          {market?.vettedError ? <Notice tone="warn">The community list could not be read: {market.vettedError}</Notice> : null}
          {preview ? (
            <div className="flex flex-col gap-2.5 rounded-[14px] border border-line-strong bg-plate-2 p-4">
              <span className="text-sm font-medium">{preview.name}</span>
              {preview.description ? <p className="m-0 text-[13px] text-ink-2">{preview.description}</p> : null}
              <span className="font-mono text-[10px] text-faint">{preview.url}</span>
              <p className="m-0 text-[13px] text-ink-2">
                {preview.pluginCount === 0
                  ? "It lists no plugins yet."
                  : `${preview.pluginCount === 1 ? "1 plugin" : `${preview.pluginCount} plugins`}: ${preview.plugins.slice(0, 6).join(", ")}${preview.plugins.length > 6 ? "…" : ""}`}
              </p>
              {preview.kind === "custom" ? (
                <Notice tone="warn">FlexDeck has not reviewed this repository. Its plugins run as programs on your desktop — add it only if you trust whoever runs it.</Notice>
              ) : null}
              <div className="flex gap-2">
                <Key
                  tone="signal"
                  size="sm"
                  disabled={busy}
                  onClick={() =>
                    void act(async () => {
                      await api.addRepo(desktopId, preview.url);
                      setPreview(null);
                      setAddress("");
                    })
                  }
                >
                  Add repository
                </Key>
                <Key size="sm" disabled={busy} onClick={() => setPreview(null)}>
                  Cancel
                </Key>
              </div>
            </div>
          ) : (
            <form
              className="flex gap-2"
              onSubmit={(event) => {
                event.preventDefault();
                if (!address.trim()) return;
                void act(async () => setPreview(await api.previewRepo(desktopId, address)));
              }}
            >
              <input
                type="text"
                value={address}
                aria-label="Repository address"
                placeholder="Add one: example.com or …/repository.min.json"
                spellCheck={false}
                onChange={(event) => setAddress(event.currentTarget.value)}
                className={cn(control, "h-10 font-mono text-[12px]")}
              />
              <Key size="md" type="submit" disabled={busy || address.trim() === ""}>
                Check
              </Key>
            </form>
          )}
        </Card>
      </div>

      <Card
        title="Marketplace"
        actions={
          <button
            type="button"
            disabled={busy}
            onClick={() => void act(() => api.refreshRepos(desktopId))}
            className="flex cursor-pointer items-center gap-1.5 font-mono text-[10px] tracking-[0.12em] text-muted uppercase hover:text-ink disabled:opacity-50"
          >
            <Glyph name="refresh" size={12} /> {read ? `Read ${read}` : "Refresh"}
          </button>
        }
      >
        <Segmented
          label="Type"
          size="sm"
          value={type}
          onChange={(next) => {
            setType(next);
            setFilter("all");
          }}
          options={MARKET_TYPES.map((option) => ({
            value: option.value,
            label: `${option.label} · ${option.value === "iconPacks" ? packs.length : entries.length}`,
          }))}
          className="self-start"
        />
        <div className="relative">
          <Glyph name="search" className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-faint" />
          <input
            type="search"
            value={query}
            aria-label={type === "iconPacks" ? "Search icon packs" : "Search plugins"}
            placeholder={type === "iconPacks" ? "Search icon packs…" : "Search OBS, lights, Spotify…"}
            onChange={(event) => setQuery(event.currentTarget.value)}
            className={cn(control, "h-10 pl-9 text-sm")}
          />
        </div>
        <div className="flex flex-wrap gap-1.5" role="tablist" aria-label="Show">
          {FILTERS.filter(({ filter: chip }) => chip === "all" || counts[chip] > 0).map(({ filter: chip, label }) => (
            <button
              key={chip}
              type="button"
              role="tab"
              aria-selected={filter === chip}
              onClick={() => setFilter(chip)}
              className={cn(
                "h-7 cursor-pointer rounded-full border px-2.5 font-mono text-[10px] tracking-[0.12em] uppercase transition-colors",
                filter === chip ? "border-ink bg-ink text-body" : "border-line text-muted hover:text-ink",
                chip === "updates" && filter !== chip && "border-signal-line text-signal",
              )}
            >
              {label} {counts[chip]}
            </button>
          ))}
        </div>
        {market === null ? (
          <p className="m-0 text-sm text-muted">The desktop is reading its repositories…</p>
        ) : type === "iconPacks" ? (
          shownPacks.length === 0 ? (
            <p className="m-0 text-sm text-muted">{packs.length === 0 ? "No icon packs listed yet." : "Nothing matches."}</p>
          ) : (
            <motion.ul variants={list} initial="initial" animate="animate" className="m-0 grid list-none gap-2.5 p-0 lg:grid-cols-2">
              {shownPacks.map((pack) => (
                <motion.li key={`${pack.repo.url} ${pack.id}`} variants={item} className="flex flex-col gap-2.5 overflow-hidden rounded-[16px] border border-line bg-body p-3">
                  <div className="flex items-center gap-3">
                    <PluginIcon entry={{ name: pack.name, iconData: null }} />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm font-medium">{pack.name}</span>
                      <span className="block truncate font-mono text-[10px] text-muted">
                        {pack.version} · {pack.count === 1 ? "1 icon" : `${pack.count} icons`}
                      </span>
                    </span>
                    <Key
                      size="sm"
                      tone={pack.installed ? "default" : "signal"}
                      disabled={busy || pack.installed}
                      onClick={() => void act(() => api.installRepositoryIconPack(desktopId, pack.repo.url, pack.id))}
                    >
                      {pack.installed ? "Installed" : "Install"}
                    </Key>
                  </div>
                  {pack.description ? <p className="m-0 line-clamp-2 text-[13px] leading-relaxed text-ink-2">{pack.description}</p> : null}
                  <div className="flex flex-wrap gap-1.5">
                    <span className="rounded-md border border-line px-1.5 py-0.5 font-mono text-[9px] tracking-[0.12em] text-muted uppercase" title={pack.repo.name}>
                      {pack.repo.kind === "official" ? "Official" : pack.repo.kind === "vetted" ? pack.repo.name : `Custom · ${pack.repo.name}`}
                    </span>
                  </div>
                </motion.li>
              ))}
            </motion.ul>
          )
        ) : shown.length === 0 ? (
          <p className="m-0 text-sm text-muted">{entries.length === 0 ? "No plugins listed yet." : "Nothing matches."}</p>
        ) : (
          <motion.ul variants={list} initial="initial" animate="animate" className="m-0 grid list-none gap-2.5 p-0 lg:grid-cols-2">
            {shown.map((entry) => {
              const key = `${entry.repo.url} ${entry.id}`;
              const action = entryAction(entry);
              const updated = relativeDate(entry.latestUpdateDate);
              const isOpen = open === key;
              return (
                <motion.li
                  key={key}
                  variants={item}
                  className={cn("flex flex-col gap-2.5 overflow-hidden rounded-[16px] border bg-body p-3", entry.update ? "border-signal-line" : "border-line")}
                >
                  <div className="flex items-center gap-3">
                    <button type="button" aria-expanded={isOpen} onClick={() => setOpen(isOpen ? null : key)} className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left">
                      <PluginIcon entry={entry} />
                      <span className="min-w-0">
                        <span className="block truncate text-sm font-medium">{entry.name}</span>
                        <span className="block truncate font-mono text-[10px] text-muted">
                          {entry.update && entry.installedVersion ? `${entry.installedVersion} → ` : ""}
                          {entry.version}
                          {updated ? ` · ${updated}` : ""}
                        </span>
                      </span>
                    </button>
                    <Key size="sm" tone={action.primary ? "signal" : "default"} disabled={busy || !action.enabled} onClick={() => {
                      setError(null);
                      setPending(pendingFromEntry(entry));
                    }}>
                      {action.label}
                    </Key>
                  </div>
                  {entry.description ? <p className={cn("m-0 text-[13px] leading-relaxed text-ink-2", !isOpen && "line-clamp-2")}>{entry.description}</p> : null}
                  <div className="flex flex-wrap gap-1.5">
                    <span className="rounded-md border border-line px-1.5 py-0.5 font-mono text-[9px] tracking-[0.12em] text-muted uppercase" title={entry.repo.name}>
                      {entry.repo.kind === "official" ? "Official" : entry.repo.kind === "vetted" ? entry.repo.name : `Custom · ${entry.repo.name}`}
                    </span>
                    {entry.permissions.map((permission) => (
                      <span key={permission} title={permissionLabel(permission)} className="rounded-md border border-line px-1.5 py-0.5 font-mono text-[9px] tracking-[0.12em] text-signal uppercase">
                        {permission}
                      </span>
                    ))}
                  </div>
                  <AnimatePresence initial={false}>
                    {isOpen ? (
                      <motion.dl
                        initial={{ height: 0, opacity: 0 }}
                        animate={{ height: "auto", opacity: 1 }}
                        exit={{ height: 0, opacity: 0 }}
                        transition={spring.panel}
                        className="m-0 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 font-mono text-[11px]"
                      >
                        {entry.releaseDate ? (
                          <>
                            <dt className="text-faint">FIRST</dt>
                            <dd className="m-0 text-ink-2">{new Date(entry.releaseDate).toLocaleDateString()}</dd>
                          </>
                        ) : null}
                        <dt className="text-faint">FROM</dt>
                        <dd className="m-0 text-ink-2">
                          {entry.repo.name} ({kindLabel(entry.repo.kind).toLowerCase()})
                        </dd>
                        {entry.homepage ? (
                          <>
                            <dt className="text-faint">WEB</dt>
                            <dd className="m-0 truncate">
                              <a href={entry.homepage} target="_blank" rel="noreferrer noopener" className="text-signal">
                                {entry.homepage}
                              </a>
                            </dd>
                          </>
                        ) : null}
                      </motion.dl>
                    ) : null}
                  </AnimatePresence>
                </motion.li>
              );
            })}
          </motion.ul>
        )}
      </Card>

      <Sheet open={pending !== null} onClose={() => setPending(null)} label="Install a plugin" placement="viewport">
        {pending ? (
          <div className="flex flex-col gap-4">
            <div className="flex items-center gap-3.5">
              <PluginIcon entry={pending} large />
              <div className="min-w-0">
                <span className="lbl">Install · from {pending.from}</span>
                <h3 className="m-0 mt-1 truncate text-xl font-semibold">
                  {pending.name} <span className="font-mono text-xs text-faint">{pending.version}</span>
                </h3>
                {pending.author ? <p className="m-0 text-[12px] text-muted">by {pending.author}</p> : null}
              </div>
            </div>
            {pending.description ? <p className="m-0 text-[13px] leading-relaxed text-ink-2">{pending.description}</p> : null}
            {pending.replaces ? <p className="m-0 text-[12px] text-muted">Replaces version {pending.replaces}. Its settings are kept.</p> : null}
            <div className="flex flex-col gap-2">
              <span className="lbl">{pending.permissions.length > 0 ? "It will be able to" : "It asks for no permissions"}</span>
              {pending.permissions.map((permission) => (
                <div key={permission} className="flex items-start gap-2.5 rounded-lg border border-line bg-body px-3 py-2 text-[13px]">
                  <span className="mt-0.5 font-mono text-[10px] tracking-[0.12em] text-signal uppercase">{permission}</span>
                  <span className="text-ink-2">{permissionLabel(permission)}</span>
                </div>
              ))}
            </div>
            {pending.unreviewed === "repository" ? (
              <Notice tone="warn">
                From <strong>{pending.from}</strong>, a repository added by hand. FlexDeck has not reviewed it; install only if you trust whoever runs it.
              </Notice>
            ) : pending.unreviewed === "file" ? (
              <Notice tone="warn">From a file, not a repository: nothing vouches for it but where you got it.</Notice>
            ) : null}
            <p className="m-0 text-[12px] text-muted">
              {pending.source.kind === "repository"
                ? "Plugins run as programs on your desktop. It checks the download matches what the repository listed before running anything."
                : "Plugins run as programs on your desktop."}
            </p>
            {error ? <Notice>{error}</Notice> : null}
            <div className="flex gap-2.5">
              <Key size="lg" className="flex-1" disabled={busy} onClick={() => setPending(null)}>
                Cancel
              </Key>
              <HoldToConfirm tone="signal" size="lg" className="flex-[2]" disabled={busy} onConfirm={() => void install(pending)}>
                Hold to install
              </HoldToConfirm>
            </div>
          </div>
        ) : null}
      </Sheet>
    </div>
  );
}

function PluginIcon({ entry, large = false }: { entry: Pick<CatalogEntry, "name" | "iconData">; large?: boolean }) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "flex shrink-0 items-center justify-center overflow-hidden rounded-[11px] border border-[var(--fd-key-line)] bg-linear-to-b from-[var(--fd-key-top)] to-[var(--fd-key-bottom)] font-mono font-bold text-ink-2",
        large ? "size-14 text-lg" : "size-10 text-[13px]",
      )}
    >
      {entry.iconData ? <img src={entry.iconData} alt="" className="size-full object-cover" /> : entry.name.trim().slice(0, 2).toUpperCase() || "?"}
    </span>
  );
}

function InstalledRow({
  plugin,
  settings,
  guide,
  busy,
  onEnable,
  onReload,
  onRemove,
  onSave,
  onSignOut,
  onLog,
}: {
  plugin: InstalledPlugin;
  settings: PluginSetting[];
  guide: string | null;
  busy: boolean;
  onEnable: (enabled: boolean) => void;
  onReload: () => void;
  onRemove: () => void;
  onSave: (values: Record<string, string>) => void;
  onSignOut: () => void;
  onLog: () => Promise<string[]>;
}): ReactElement {
  const [open, setOpen] = useState<"settings" | "log" | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [log, setLog] = useState<string[]>([]);
  const state = pluginState(plugin);

  return (
    <motion.li variants={item} className="overflow-hidden rounded-[14px] border border-line bg-body">
      <div className="flex items-center gap-3 p-3">
        <button type="button" aria-expanded={expanded} onClick={() => setExpanded(!expanded)} className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left">
          <PluginIcon entry={{ name: plugin.name, iconData: null }} />
          <span className="min-w-0">
            <span className="block truncate text-sm font-medium">
              {plugin.name} <span className="font-mono text-[10px] text-faint">{plugin.version}</span>
            </span>
            <span className="flex items-center gap-1.5 font-mono text-[10px] tracking-[0.1em] text-muted uppercase">
              <Led state={state.tone === "ok" ? "ok" : state.tone === "warn" ? "warn" : "off"} size="sm" /> {state.word}
            </span>
          </span>
        </button>
        <Switch checked={plugin.enabled} disabled={busy} label={`${plugin.enabled ? "Turn off" : "Turn on"} ${plugin.name}`} onChange={onEnable} />
      </div>
      <AnimatePresence initial={false}>
        {expanded ? (
          <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: "auto", opacity: 1 }} exit={{ height: 0, opacity: 0 }} transition={spring.panel}>
            <div className="flex flex-col gap-3 border-t border-dashed border-line p-3">
              {state.reason ? <p className="m-0 text-[13px] text-warn">{state.reason}</p> : null}
              <div className="flex flex-wrap gap-2">
                {settings.length > 0 ? (
                  <Key size="sm" lit={open === "settings"} onClick={() => setOpen(open === "settings" ? null : "settings")}>
                    <Glyph name="sliders" size={13} /> Settings
                  </Key>
                ) : null}
                {guide ? (
                  <a href={guide} target="_blank" rel="noreferrer noopener" className={cn(keyVariants({ size: "sm" }), "no-underline")}>
                    <Glyph name="book" size={13} /> Setup guide
                  </a>
                ) : null}
                <Key
                  size="sm"
                  lit={open === "log"}
                  onClick={() => {
                    const next = open === "log" ? null : "log";
                    setOpen(next);
                    if (next) void onLog().then(setLog, () => setLog(["The log could not be read."]));
                  }}
                >
                  <Glyph name="log" size={13} /> Log
                </Key>
                {plugin.enabled ? (
                  <Key size="sm" disabled={busy} onClick={onReload}>
                    <Glyph name="refresh" size={13} /> Restart
                  </Key>
                ) : null}
                <HoldToConfirm size="sm" tone="danger" className="ml-auto" disabled={busy} onConfirm={onRemove}>
                  Hold to remove
                </HoldToConfirm>
              </div>
              {open === "settings" ? (
                <SettingsForm
                  settings={settings}
                  busy={busy}
                  onSave={(values) => {
                    onSave(values);
                    setOpen(null);
                  }}
                  onSignOut={onSignOut}
                />
              ) : null}
              {open === "log" ? (
                <pre className="m-0 max-h-48 overflow-auto rounded-lg border border-line bg-well p-2.5 font-mono text-[11px] leading-relaxed text-ink-2 shadow-well">
                  {log.length > 0 ? log.join("\n") : "Nothing logged yet."}
                </pre>
              ) : null}
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </motion.li>
  );
}

/**
 * A form built from what the plugin declared. A secret is never shown again.
 * What the plugin keeps for itself (a sign-in) is not a field: the desktop
 * keeps it across saves, and signing out clears it.
 */
function SettingsForm({
  settings: all,
  busy,
  onSave,
  onSignOut,
}: {
  settings: PluginSetting[];
  busy: boolean;
  onSave: (values: Record<string, string>) => void;
  onSignOut: () => void;
}): ReactElement {
  const settings = all.filter((setting) => !setting.internal);
  const signedIn = all.some((setting) => setting.internal && setting.isSet);
  const [values, setValues] = useState<Record<string, string>>(() =>
    Object.fromEntries(settings.map((setting) => [setting.key, setting.secret ? "" : (setting.value ?? "")])),
  );
  return (
    <form
      className="flex flex-col gap-3 rounded-xl border border-line bg-plate p-3"
      onSubmit={(event) => {
        event.preventDefault();
        onSave(Object.fromEntries(Object.entries(values).filter(([, value]) => value !== "")));
      }}
    >
      {settings.map((setting) => (
        <label key={setting.key} className="flex flex-col gap-1.5">
          <span className="inline-flex items-center gap-1 text-[12px] text-muted">
            {setting.description || setting.key}
            {setting.help ? <InfoTip label={`About ${setting.description || setting.key}`}>{setting.help}</InfoTip> : null}
          </span>
          {setting.kind === "choice" ? (
            <select className={control} value={values[setting.key]} onChange={(event) => setValues({ ...values, [setting.key]: event.currentTarget.value })}>
              <option value="">Default ({setting.defaultValue ?? "none"})</option>
              {setting.choices.map((choice) => (
                <option key={choice}>{choice}</option>
              ))}
            </select>
          ) : (
            <input
              className={control}
              type={setting.secret ? "password" : setting.kind === "number" ? "number" : "text"}
              value={values[setting.key]}
              autoComplete="off"
              placeholder={setting.secret ? (setting.isSet ? "Saved — leave empty to keep it" : "Not set") : (setting.defaultValue ?? "")}
              onChange={(event) => setValues({ ...values, [setting.key]: event.currentTarget.value })}
            />
          )}
        </label>
      ))}
      {signedIn ? (
        <div className="flex items-center justify-between gap-3 rounded-lg border border-line bg-well px-3 py-2">
          <span className="text-[13px] text-ink-2">Signed in</span>
          <Key size="sm" type="button" disabled={busy} onClick={onSignOut}>
            Sign out
          </Key>
        </div>
      ) : null}
      {settings.length > 0 ? (
        <Key tone="signal" size="sm" type="submit" disabled={busy} className="self-start">
          Save settings
        </Key>
      ) : null}
    </form>
  );
}
