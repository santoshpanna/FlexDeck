import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";

import {
  Field,
  Glyph,
  HoldToConfirm,
  Input,
  Key,
  Label,
  Led,
  Readout,
  Segmented,
  Select,
  Switch,
  cn,
  item,
  list,
  screen,
  spring,
} from "@flexdeck/ui";

import {
  entryAction,
  filterCatalog,
  filterCounts,
  formatSize,
  kindLabel,
  permissionLabel,
  pluginState,
  relativeDate,
  sourceLabel,
  type MarketFilter,
} from "../../lib/plugins.lib.ts";
import type { CatalogEntry, InstalledPlugin, PluginSetting } from "../../lib/types.ts";
import type { ShellViewProps } from "../../view/shell.view.tsx";
import { PluginFace } from "../PluginFace/PluginFace.component.tsx";

type View = "installed" | "browse";

const FILTERS: { filter: MarketFilter; label: string }[] = [
  { filter: "all", label: "All" },
  { filter: "official", label: "Official" },
  { filter: "vetted", label: "Community" },
  { filter: "custom", label: "Custom" },
  { filter: "updates", label: "Updates" },
];

/**
 * What this computer can do: the plugins it has, and the ones it could have.
 * One screen with two views rather than two cards, so installing something
 * lands you looking at it in the list you came from.
 */
export function PluginsScreen({ p }: { p: ShellViewProps }) {
  const [view, setView] = useState<View>(p.installed.length === 0 ? "browse" : "installed");
  const updates = (p.catalog ?? []).filter((entry) => entry.update).length;

  return (
    <div className="flex flex-col gap-3.5">
      <div className="flex items-center justify-between">
        <Segmented
          label="Plugins view"
          value={view}
          onChange={setView}
          options={[
            { value: "installed", label: `Installed · ${p.installed.length}` },
            {
              value: "browse",
              label: updates > 0 ? `Browse · ${updates} new` : "Browse",
            },
          ]}
        />
      </div>
      <AnimatePresence mode="wait" initial={false}>
        <motion.div key={view} variants={screen} initial="initial" animate="animate" exit="exit">
          {view === "installed" ? <Installed p={p} onBrowse={() => setView("browse")} /> : <Browse p={p} />}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}

function Installed({ p, onBrowse }: { p: ShellViewProps; onBrowse: () => void }) {
  return (
    <div className="flex flex-col gap-4">
      {p.installed.length === 0 ? (
        <div className="flex flex-col items-start gap-3 rounded-xl border border-dashed border-line-strong p-4">
          <p className="m-0 text-[13px] text-muted">
            Nothing installed yet. OBS, Spotify, Discord and more are a tap away.
          </p>
          <Key tone="signal" size="md" onClick={onBrowse}>
            Browse plugins
          </Key>
        </div>
      ) : (
        <motion.ul variants={list} initial="initial" animate="animate" className="flex flex-col gap-2">
          {p.installed.map((plugin) => (
            <InstalledRow key={plugin.id} plugin={plugin} p={p} />
          ))}
        </motion.ul>
      )}

      {p.plugins.length > 0 ? (
        <section className="flex flex-col gap-2" aria-label="Built in">
          <Label className="px-0.5">Built in</Label>
          <div className="grid grid-cols-2 gap-2">
            {p.plugins.map((plugin) => (
              <div
                key={plugin.name}
                className="flex items-center gap-2.5 rounded-xl border border-line bg-plate px-3 py-2.5"
              >
                <Led state="ok" />
                <div className="min-w-0">
                  <div className="truncate text-[13px]">{plugin.name}</div>
                  <Readout className="text-[10px]">{plugin.detail}</Readout>
                </div>
              </div>
            ))}
          </div>
        </section>
      ) : null}
    </div>
  );
}

function InstalledRow({ plugin, p }: { plugin: InstalledPlugin; p: ShellViewProps }) {
  const [open, setOpen] = useState<"settings" | "log" | null>(null);
  const [expanded, setExpanded] = useState(false);
  const state = pluginState(plugin);
  const settings = plugin.settings ?? [];
  const log = p.logs[plugin.id];

  return (
    <motion.li variants={item} className="overflow-hidden rounded-[14px] border border-line bg-plate">
      <div className="flex items-center gap-3 p-3">
        <button
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded(!expanded)}
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left"
        >
          <PluginFace name={plugin.name} />
          <span className="min-w-0 flex-1">
            <span className="block truncate text-sm font-medium">
              {plugin.name} <span className="font-mono text-[10px] text-faint">{plugin.version}</span>
            </span>
            <span className="flex items-center gap-1.5 font-mono text-[10px] tracking-[0.1em] text-muted uppercase">
              <Led state={state.tone === "ok" ? "ok" : state.tone === "warn" ? "warn" : "off"} size="sm" />
              {state.word}
              {plugin.actions !== undefined ? ` · ${plugin.actions} actions` : ""}
            </span>
          </span>
        </button>
        <Switch
          checked={plugin.enabled}
          disabled={p.busy}
          label={`${plugin.enabled ? "Turn off" : "Turn on"} ${plugin.name}`}
          onChange={(enabled) => p.onEnable(plugin.id, enabled)}
        />
      </div>

      <AnimatePresence initial={false}>
        {expanded ? (
          <motion.div
            key="more"
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={spring.panel}
          >
            <div className="border-t border-dashed border-line px-3 pt-3 pb-3">
              <div className="flex flex-col gap-3">
                <Readout className="text-[10px] tracking-[0.06em] normal-case">{sourceLabel(plugin)}</Readout>
                {state.reason ? <p className="m-0 text-[13px] text-warn">{state.reason}</p> : null}
                <div className="flex flex-wrap gap-2">
                  {settings.length > 0 ? (
                    <Key
                      size="sm"
                      lit={open === "settings"}
                      onClick={() => setOpen(open === "settings" ? null : "settings")}
                      disabled={p.busy}
                    >
                      <Glyph name="sliders" size={14} /> Settings
                    </Key>
                  ) : null}
                  <Key
                    size="sm"
                    lit={open === "log"}
                    onClick={() => {
                      setOpen(open === "log" ? null : "log");
                      p.onShowLog(plugin.id);
                    }}
                  >
                    <Glyph name="log" size={14} /> Log
                  </Key>
                  {plugin.enabled ? (
                    <Key size="sm" onClick={() => p.onReload(plugin.id)} disabled={p.busy}>
                      <Glyph name="refresh" size={14} /> Restart
                    </Key>
                  ) : null}
                  <HoldToConfirm
                    size="sm"
                    tone="danger"
                    className="ml-auto"
                    disabled={p.busy}
                    onConfirm={() => p.onUninstall(plugin.id)}
                  >
                    Hold to remove
                  </HoldToConfirm>
                </div>
              </div>
              {/* Outside the gap column, with their own top padding: a panel
                  folding away to zero height then takes its spacing with it
                  instead of leaving a gap that snaps shut at the end. */}
              <AnimatePresence initial={false}>
                {open ? (
                  <motion.div
                    key={open}
                    initial={{ height: 0, opacity: 0 }}
                    animate={{ height: "auto", opacity: 1 }}
                    exit={{ height: 0, opacity: 0 }}
                    transition={spring.panel}
                    className="overflow-hidden"
                  >
                    <div className="pt-3">
                      {open === "settings" ? (
                        <SettingsForm
                          settings={settings}
                          busy={p.busy}
                          onSave={(values) => {
                            p.onSaveSettings(plugin.id, values);
                            setOpen(null);
                          }}
                        />
                      ) : (
                        <pre className="m-0 max-h-48 overflow-auto rounded-lg border border-line bg-well p-2.5 font-mono text-[11px] leading-relaxed text-ink-2 shadow-well">
                          {(log ?? []).length > 0 ? (log ?? []).join("\n") : "Nothing logged yet."}
                        </pre>
                      )}
                    </div>
                  </motion.div>
                ) : null}
              </AnimatePresence>
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </motion.li>
  );
}

/**
 * A form generated from what the plugin declared. A secret is never shown —
 * the agent does not send it back — so its box starts empty and saying nothing
 * keeps the stored one.
 */
function SettingsForm({
  settings,
  busy,
  onSave,
}: {
  settings: PluginSetting[];
  busy: boolean;
  onSave: (values: Record<string, string>) => void;
}) {
  const [values, setValues] = useState<Record<string, string>>(() =>
    Object.fromEntries(settings.map((setting) => [setting.key, setting.secret ? "" : (setting.value ?? "")])),
  );

  return (
    <form
      className="flex flex-col gap-3 rounded-xl border border-line bg-body p-3"
      onSubmit={(event) => {
        event.preventDefault();
        // An empty field means "use the default" for an ordinary setting, and
        // "keep what is stored" for a secret, so neither is sent as "".
        onSave(Object.fromEntries(Object.entries(values).filter(([, value]) => value !== "")));
      }}
    >
      {settings.map((setting) => {
        const id = `setting-${setting.key}`;
        return (
          <Field key={setting.key} label={setting.description || setting.key} htmlFor={id}>
            {setting.kind === "choice" ? (
              <Select
                id={id}
                value={values[setting.key]}
                disabled={busy}
                onChange={(event) => setValues({ ...values, [setting.key]: event.target.value })}
              >
                <option value="">Default ({setting.defaultValue ?? "none"})</option>
                {setting.choices.map((choice) => (
                  <option key={choice} value={choice}>
                    {choice}
                  </option>
                ))}
              </Select>
            ) : (
              <Input
                id={id}
                type={setting.secret ? "password" : setting.kind === "number" ? "number" : "text"}
                value={values[setting.key]}
                placeholder={
                  setting.secret
                    ? setting.isSet
                      ? "Saved — leave empty to keep it"
                      : "Not set"
                    : (setting.defaultValue ?? "")
                }
                autoComplete="off"
                spellCheck={false}
                disabled={busy}
                onChange={(event) => setValues({ ...values, [setting.key]: event.target.value })}
              />
            )}
          </Field>
        );
      })}
      <Key tone="signal" size="md" type="submit" disabled={busy}>
        Save settings
      </Key>
    </form>
  );
}

/**
 * The marketplace. Search and the chips filter what is already loaded, so
 * typing is instant and never touches the network. Nothing installs from
 * here: picking one opens the consent sheet.
 */
function Browse({ p }: { p: ShellViewProps }) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<MarketFilter>("all");
  const [open, setOpen] = useState<string | null>(null);
  const entries = p.catalog ?? [];
  const counts = filterCounts(entries);
  const shown = filterCatalog(entries, query, filter);
  const read = relativeDate(p.fetchedAt);

  return (
    <div className="flex flex-col gap-3">
      <div className="relative">
        <Glyph name="search" className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-faint" />
        <Input
          type="search"
          aria-label="Search plugins"
          placeholder="Search OBS, lights, Spotify…"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          className="pl-9"
        />
      </div>
      <div className="flex flex-wrap items-center gap-1.5" role="tablist" aria-label="Show">
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
        <button
          type="button"
          onClick={p.onRefresh}
          disabled={p.busy}
          title="Read every repository again"
          className="ml-auto flex cursor-pointer items-center gap-1.5 font-mono text-[10px] tracking-[0.1em] text-muted uppercase hover:text-ink disabled:opacity-50"
        >
          <Glyph name="refresh" size={12} /> {read ? read : "Refresh"}
        </button>
      </div>

      {p.catalog === null ? (
        <p className="m-0 text-[13px] text-muted">Reading the repositories…</p>
      ) : shown.length === 0 ? (
        <p className="m-0 text-[13px] text-muted">
          {entries.length === 0 ? "No plugins listed yet." : "Nothing matches."}
        </p>
      ) : (
        <motion.ul variants={list} initial="initial" animate="animate" className="flex flex-col gap-2">
          {shown.map((entry) => {
            const key = `${entry.repo.url} ${entry.id}`;
            return (
              <CatalogRow
                key={key}
                entry={entry}
                open={open === key}
                busy={p.busy}
                onToggle={() => setOpen(open === key ? null : key)}
                onPick={p.onPick}
              />
            );
          })}
        </motion.ul>
      )}

      <OtherWays busy={p.busy} onChooseFile={p.onChooseFile} onLink={p.onLink} />
    </div>
  );
}

function CatalogRow({
  entry,
  open,
  busy,
  onToggle,
  onPick,
}: {
  entry: CatalogEntry;
  open: boolean;
  busy: boolean;
  onToggle: () => void;
  onPick: (entry: CatalogEntry) => void;
}) {
  const action = entryAction(entry);
  const updated = relativeDate(entry.latestUpdateDate);
  return (
    <motion.li
      variants={item}
      className={cn(
        "overflow-hidden rounded-[14px] border bg-plate",
        entry.update ? "border-signal-line" : "border-line",
      )}
    >
      <div className="flex items-center gap-3 p-3">
        <button
          type="button"
          aria-expanded={open}
          onClick={onToggle}
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left"
        >
          <PluginFace name={entry.name} icon={entry.iconData} />
          <span className="min-w-0 flex-1">
            <span className="block truncate text-sm font-medium">{entry.name}</span>
            <span className="block truncate font-mono text-[10px] tracking-[0.06em] text-muted">
              {entry.update && entry.installedVersion ? `${entry.installedVersion} → ` : ""}
              {entry.version} · {kindLabel(entry.repo.kind).toUpperCase()}
              {updated ? ` · ${updated}` : ""}
            </span>
          </span>
        </button>
        <Key
          size="sm"
          tone={action.primary ? "signal" : "default"}
          disabled={busy || !action.enabled}
          onClick={() => onPick(entry)}
        >
          {action.label}
        </Key>
      </div>
      <AnimatePresence initial={false}>
        {open ? (
          <motion.div
            key="more"
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={spring.panel}
          >
            <div className="flex flex-col gap-2.5 border-t border-dashed border-line px-3 pt-3 pb-3 text-[13px]">
              {entry.description ? <p className="m-0 leading-relaxed text-ink-2">{entry.description}</p> : null}
              {entry.permissions.length > 0 ? (
                <div className="flex flex-wrap gap-1.5">
                  {entry.permissions.map((permission) => (
                    <span
                      key={permission}
                      title={permissionLabel(permission)}
                      className="rounded-full border border-line bg-body px-2 py-1 font-mono text-[10px] tracking-[0.1em] text-muted uppercase"
                    >
                      {permission}
                    </span>
                  ))}
                </div>
              ) : null}
              <dl className="m-0 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 font-mono text-[11px]">
                {entry.author ? (
                  <>
                    <dt className="text-faint">BY</dt>
                    <dd className="m-0 text-ink-2">{entry.author}</dd>
                  </>
                ) : null}
                <dt className="text-faint">FROM</dt>
                <dd className="m-0 text-ink-2">{entry.repo.name}</dd>
                {entry.package.size > 0 ? (
                  <>
                    <dt className="text-faint">SIZE</dt>
                    <dd className="m-0 text-ink-2">{formatSize(entry.package.size)}</dd>
                  </>
                ) : null}
                {entry.homepage ? (
                  <>
                    <dt className="text-faint">WEB</dt>
                    <dd className="m-0 truncate text-ink-2">{entry.homepage}</dd>
                  </>
                ) : null}
                {entry.installedVersion && !entry.installedFromHere ? (
                  <>
                    <dt className="text-faint">HAVE</dt>
                    <dd className="m-0 text-ink-2">{entry.installedVersion}, from another source</dd>
                  </>
                ) : null}
              </dl>
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </motion.li>
  );
}

function OtherWays({
  busy,
  onChooseFile,
  onLink,
}: {
  busy: boolean;
  onChooseFile: (file: File) => void;
  onLink: (path: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [folder, setFolder] = useState("");
  return (
    <div className="rounded-[14px] border border-dashed border-line-strong">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="flex w-full cursor-pointer items-center justify-between px-3 py-2.5 text-left"
      >
        <Label>Other ways to install</Label>
        <motion.span animate={{ rotate: open ? 180 : 0 }} transition={spring.panel} className="text-muted">
          <Glyph name="chevronDown" size={14} />
        </motion.span>
      </button>
      <AnimatePresence initial={false}>
        {open ? (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={spring.panel}
          >
            <div className="flex flex-col gap-3.5 px-3 pb-3">
              <Field label="From a file (.fdpkg)" htmlFor="plugin-file">
                <input
                  id="plugin-file"
                  type="file"
                  accept=".fdpkg"
                  disabled={busy}
                  className="text-[12px] text-muted file:mr-3 file:cursor-pointer file:rounded-lg file:border file:border-line file:bg-plate-2 file:px-3 file:py-1.5 file:text-ink"
                  onChange={(event) => {
                    const file = event.target.files?.[0];
                    if (file) onChooseFile(file);
                    event.target.value = "";
                  }}
                />
              </Field>
              <Field
                label="Link a folder"
                htmlFor="plugin-folder"
                hint="Writing a plugin? Run it from its folder; it reloads every time you save."
              >
                <div className="flex gap-2">
                  <Input
                    id="plugin-folder"
                    value={folder}
                    placeholder="C:\code\my-plugin"
                    spellCheck={false}
                    disabled={busy}
                    onChange={(event) => setFolder(event.target.value)}
                  />
                  <Key size="md" disabled={busy || folder.trim() === ""} onClick={() => onLink(folder)}>
                    <Glyph name="folder" /> Link
                  </Key>
                </div>
              </Field>
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}
