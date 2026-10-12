import { AnimatePresence, motion } from "motion/react";
import { useEffect, useState, type ReactNode } from "react";

import type { ThemePreference } from "@flexdeck/tokens/theme.ts";
import { Field, HoldToConfirm, Input, Key, Label, Led, Notice, Plate, Readout, Segmented, Switch, cn, item, list, spring } from "@flexdeck/ui";

import { useTheme } from "../../hooks/useTheme.hook.ts";
import { kindLabel, relativeDate } from "../../lib/plugins.lib.ts";
import { webAccessSummary } from "../../lib/status.lib.ts";
import type { WindowPrefs } from "../../lib/types.ts";
import type { ShellViewProps } from "../../view/shell.view.tsx";

/** Everything that is set once and then left alone. */
export function SetupScreen({ p }: { p: ShellViewProps }) {
  return (
    <motion.div variants={list} initial="initial" animate="animate" className="flex flex-col gap-5">
      <Section title="Web access">
        <WebAccess p={p} />
      </Section>
      <Section title={`Repositories · ${p.repositories.length}`}>
        <Repositories p={p} />
      </Section>
      <Section title="Startup">
        <Startup p={p} />
      </Section>
      <Section title="Appearance">
        <Appearance />
      </Section>
      <Section title="This computer">
        <Plate depth="well" pad="sm" className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 font-mono text-[11px]">
          <span className="text-faint">ADDRESS</span>
          <span className="truncate text-ink-2">{p.address || "—"}</span>
          <span className="text-faint">FINGERPRINT</span>
          <span className="truncate text-ink-2" title={p.fingerprint}>
            {p.fingerprint || "—"}
          </span>
        </Plate>
      </Section>
    </motion.div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <motion.section variants={item} className="flex flex-col gap-2.5" aria-label={title}>
      <Label className="px-0.5">{title}</Label>
      {children}
    </motion.section>
  );
}

/**
 * Which website to reach, and whether to reach it. The address is a draft until
 * saved, so a half-typed one never becomes the one the tunnel dials. There is no
 * account: a browser elsewhere connects with a code from the Pair tab.
 */
function WebAccess({ p }: { p: ShellViewProps }) {
  const { tunnel } = p;
  const [draft, setDraft] = useState(tunnel.portal ?? "");
  useEffect(() => setDraft(tunnel.portal ?? ""), [tunnel.portal]);
  const saved = tunnel.portal ?? "";
  const save = () => p.onSetPortal(draft);
  const live = tunnel.enabled && tunnel.running;

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-3 rounded-[14px] border border-line bg-plate p-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 text-sm">
            <Led state={tunnel.refused ? "warn" : live ? "ok" : tunnel.enabled ? "wait" : "off"} />
            Reach this PC from anywhere
          </div>
          <p className="m-0 mt-1 text-[12px] leading-relaxed text-muted">{webAccessSummary(tunnel)}</p>
        </div>
        <Switch
          checked={tunnel.enabled}
          // Nothing to connect to, so offering the switch would be a trap. The sentence says what to do first.
          disabled={p.busy || tunnel.portal === null}
          label="Web access"
          onChange={p.onToggleWebAccess}
        />
      </div>
      {tunnel.enabled && !tunnel.running && tunnel.refused === null && tunnel.lastError !== null ? (
        <Notice tone="warn">{tunnel.lastError}</Notice>
      ) : null}
      <Field label="Portal address" htmlFor="portal-address">
        <div className="flex gap-2">
          <Input
            id="portal-address"
            value={draft}
            placeholder="wss://app.flex-deck.com/tunnel"
            spellCheck={false}
            autoComplete="off"
            disabled={p.busy}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") save();
            }}
            className="font-mono text-[12px]"
          />
          <Key size="md" disabled={p.busy || draft.trim() === saved} onClick={save}>
            Save
          </Key>
        </div>
      </Field>
    </div>
  );
}

/**
 * Where the marketplace's plugins come from. Official is always there;
 * community ones were reviewed and can be switched off; anything added by
 * address is shown for what it is — unreviewed. Adding is two steps, read then
 * add, so an address is never saved on a typo.
 */
function Repositories({ p }: { p: ShellViewProps }) {
  const [address, setAddress] = useState("");
  return (
    <div className="flex flex-col gap-2.5">
      <ul className="flex flex-col gap-2">
        <AnimatePresence initial={false}>
          {p.repositories.map((repo) => (
            <motion.li
              key={repo.url}
              layout
              variants={item}
              initial="initial"
              animate="animate"
              exit="exit"
              className={cn("flex items-start gap-3 rounded-[14px] border border-line bg-plate p-3", !repo.enabled && "opacity-60")}
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="truncate text-sm">{repo.name}</span>
                  <span
                    className={cn(
                      "rounded-md border px-1.5 py-0.5 font-mono text-[9px] tracking-[0.14em] uppercase",
                      repo.kind === "custom" ? "border-warn/40 text-warn" : "border-line text-muted",
                    )}
                  >
                    {kindLabel(repo.kind)}
                  </span>
                </div>
                <Readout className="text-[10px] tracking-[0.06em] normal-case">
                  {repo.pluginCount === 1 ? "1 plugin" : `${repo.pluginCount} plugins`}
                  {repo.fetchedAt ? ` · read ${relativeDate(repo.fetchedAt)}` : ""}
                </Readout>
                {repo.kind === "custom" ? <div className="truncate font-mono text-[10px] text-faint">{repo.url}</div> : null}
                {repo.error ? (
                  <p className="m-0 mt-1 text-[12px] text-warn">
                    Could not refresh: {repo.error}
                    {repo.pluginCount > 0 ? " (showing the last copy)" : ""}
                  </p>
                ) : null}
                {repo.dropped.length > 0 ? (
                  <p className="m-0 mt-1 text-[12px] text-warn" title={repo.dropped.join("\n")}>
                    {repo.dropped.length} listed {repo.dropped.length === 1 ? "plugin is" : "plugins are"} unreadable and hidden
                  </p>
                ) : null}
              </div>
              {repo.kind === "vetted" ? (
                <Switch
                  size="sm"
                  checked={repo.enabled}
                  disabled={p.busy}
                  label={`${repo.enabled ? "Turn off" : "Turn on"} ${repo.name}`}
                  onChange={(enabled) => p.onToggleRepo(repo.url, enabled)}
                />
              ) : repo.kind === "custom" ? (
                <HoldToConfirm size="sm" tone="danger" disabled={p.busy} onConfirm={() => p.onRemoveRepo(repo.url)}>
                  Remove
                </HoldToConfirm>
              ) : null}
            </motion.li>
          ))}
        </AnimatePresence>
      </ul>
      {p.vettedError ? <Notice tone="warn">The community list could not be read: {p.vettedError}</Notice> : null}

      <AnimatePresence mode="wait" initial={false}>
        {p.preview ? (
          <motion.div key="preview" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -8 }} transition={spring.panel}>
            <Plate depth="raised" className="flex flex-col gap-2.5">
              <div className="text-sm font-medium">{p.preview.name}</div>
              {p.preview.description ? <p className="m-0 text-[13px] text-ink-2">{p.preview.description}</p> : null}
              <span className="truncate font-mono text-[10px] text-faint">{p.preview.url}</span>
              <p className="m-0 text-[13px] text-ink-2">
                {p.preview.pluginCount === 0
                  ? "It lists no plugins yet."
                  : `${p.preview.pluginCount === 1 ? "1 plugin" : `${p.preview.pluginCount} plugins`}: ${p.preview.plugins.slice(0, 6).join(", ")}${p.preview.plugins.length > 6 ? "…" : ""}`}
              </p>
              {p.preview.kind === "custom" ? (
                <Notice tone="warn">
                  FlexDeck has not reviewed this repository. Its plugins run as programs on this computer — add it only if you trust whoever runs it.
                </Notice>
              ) : null}
              <div className="flex gap-2">
                <Key
                  tone="signal"
                  size="md"
                  disabled={p.busy}
                  onClick={() => {
                    if (!p.preview) return;
                    p.onAddRepo(p.preview.url);
                    setAddress("");
                  }}
                >
                  Add repository
                </Key>
                <Key size="md" disabled={p.busy} onClick={p.onCancelPreview}>
                  Cancel
                </Key>
              </div>
            </Plate>
          </motion.div>
        ) : (
          <motion.form
            key="add"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onSubmit={(event) => {
              event.preventDefault();
              if (address.trim()) p.onPreviewRepo(address);
            }}
          >
            <Field label="Add by address" htmlFor="repo-address">
              <div className="flex gap-2">
                <Input
                  id="repo-address"
                  value={address}
                  placeholder="example.com or …/repository.min.json"
                  spellCheck={false}
                  disabled={p.busy}
                  onChange={(event) => setAddress(event.target.value)}
                  className="font-mono text-[12px]"
                />
                <Key size="md" type="submit" disabled={p.busy || address.trim() === ""}>
                  Check
                </Key>
              </div>
            </Field>
          </motion.form>
        )}
      </AnimatePresence>
    </div>
  );
}

const STARTUP: readonly { key: keyof WindowPrefs; title: string; detail: string }[] = [
  { key: "autostart", title: "Start with this computer", detail: "Open when you sign in, so decks can connect without you opening it." },
  { key: "startMinimized", title: "Start minimized", detail: "Open to the tray instead of showing this window." },
  {
    key: "closeToTray",
    title: "Close to tray",
    detail: "Closing the window keeps decks working. Off, closing quits and disconnects them.",
  },
];

/** How the window starts and closes. */
function Startup({ p }: { p: ShellViewProps }) {
  return (
    <div className="flex flex-col gap-2">
      {STARTUP.map((row) => (
        <div key={row.key} className="flex items-center gap-3 rounded-[14px] border border-line bg-plate p-3">
          <div className="min-w-0 flex-1">
            <div className="text-sm">{row.title}</div>
            <p className="m-0 mt-1 text-[12px] leading-relaxed text-muted">{row.detail}</p>
          </div>
          <Switch
            size="sm"
            checked={p.windowPrefs?.[row.key] ?? false}
            disabled={p.busy || p.windowPrefs === null}
            label={row.title}
            onChange={(value) => p.onSetWindowPrefs({ [row.key]: value })}
          />
        </div>
      ))}
    </div>
  );
}

const THEMES: readonly { value: ThemePreference; label: string }[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "System" },
];

function Appearance() {
  const { preference, setPreference } = useTheme();
  return <Segmented label="Appearance" value={preference} onChange={setPreference} options={THEMES} className="self-start" />;
}
