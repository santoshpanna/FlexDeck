import { useCallback, useEffect, useState, type ReactNode } from "react";

import { Glyph, Key, Label, Notice, Segmented, Switch, cn } from "@flexdeck/ui";

import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import type { DataSource, SourceStatus, SourceTest } from "../../../../services/api/api.ts";
import { packRecipe, recipeIdFor, recipeProblem, type RecipeMeta } from "../../lib/recipe.lib.ts";
import { STARTERS, describeJson, fieldNameFor, idFor, keptSecret, pathExpression } from "../../lib/sources.lib.ts";
import { DraftInput, control } from "../Flow/fields.tsx";

/**
 * Data sources: a URL or a program, run every so often, and the fields read
 * from what comes back — each one a value any key, rule or automation can use
 * as `{{src.<id>.<field>}}`.
 *
 * Built to be done without code: start from a working example, press Test,
 * and click the values in the answer that should become fields.
 */
export function SourcesPanel({ desktopId }: { desktopId: string }) {
  const api = useApi();
  const [saved, setSaved] = useState<DataSource[] | null>(null);
  const [draft, setDraft] = useState<DataSource[]>([]);
  const [status, setStatus] = useState<Record<string, SourceStatus>>({});
  const [secrets, setSecrets] = useState<string[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    try {
      const answer = await api.sources(desktopId);
      setSaved(answer.sources ?? []);
      setDraft(answer.sources ?? []);
      setStatus(answer.status ?? {});
      setSecrets(answer.secrets ?? []);
      setError(null);
    } catch (problem) {
      setError(String(problem instanceof Error ? problem.message : problem));
    }
  }, [api, desktopId]);

  useEffect(() => {
    void load();
    const again = setInterval(() => {
      void api
        .sources(desktopId)
        .then((answer) => setStatus(answer.status ?? {}))
        .catch(() => {});
    }, 10_000);
    return () => clearInterval(again);
  }, [api, desktopId, load]);

  const dirty = JSON.stringify(saved) !== JSON.stringify(draft);
  const current = draft.find((source) => source.id === selected) ?? null;
  const update = (id: string, change: (source: DataSource) => DataSource) =>
    setDraft((list) => list.map((source) => (source.id === id ? change(source) : source)));

  const save = async () => {
    setSaving(true);
    try {
      await api.saveSources(desktopId, draft);
      await load();
    } catch (problem) {
      setError(String(problem instanceof Error ? problem.message : problem));
    } finally {
      setSaving(false);
    }
  };

  const add = (starter: (typeof STARTERS)[number]) => {
    const id = idFor(starter.source.name ?? starter.label, draft.map((s) => s.id));
    setDraft((list) => [...list, { ...structuredClone(starter.source), id }]);
    setSelected(id);
  };

  if (saved === null && !error) {
    return <p className="text-muted">Reading the data sources…</p>;
  }

  return (
    <div className="grid gap-6 lg:grid-cols-[320px_1fr]">
      <aside className="flex flex-col gap-4">
        <div className="flex flex-col gap-1">
          <Label>Data sources</Label>
          <p className="m-0 text-[12px] text-muted">
            Live values from the web or a program. Show one on a key with <code className="font-mono">{"{{src.id.field}}"}</code>.
          </p>
        </div>
        {error ? <Notice tone="bad" onDismiss={() => setError(null)}>{error}</Notice> : null}
        <div className="flex flex-col gap-1.5">
          {draft.map((source) => {
            const run = status[`src.${source.id}`];
            return (
              <button
                key={source.id}
                type="button"
                onClick={() => setSelected(source.id)}
                className={cn(
                  "flex items-center gap-2.5 rounded-xl border px-3 py-2.5 text-left transition-colors",
                  selected === source.id ? "border-signal bg-signal-soft/40" : "border-line bg-plate hover:border-line-strong",
                )}
              >
                <span
                  aria-hidden="true"
                  className={cn("size-2 shrink-0 rounded-full", !source.enabled && source.enabled !== undefined ? "bg-faint" : run ? (run.ok ? "bg-ok" : "bg-bad") : "bg-line-strong")}
                />
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-[13px]">{source.name || source.id}</span>
                  <span className="truncate font-mono text-[11px] text-faint">src.{source.id}</span>
                </span>
                {run?.updated ? <span className="shrink-0 text-[11px] text-faint">{ago(run.updated)}</span> : null}
              </button>
            );
          })}
          {draft.length === 0 ? <p className="m-0 text-[12px] text-faint">None yet. Start from one below.</p> : null}
        </div>
        <div className="flex flex-col gap-1.5">
          <span className="lbl">Start from</span>
          {STARTERS.map((starter) => (
            <button
              key={starter.label}
              type="button"
              onClick={() => add(starter)}
              className="flex flex-col items-start gap-0.5 rounded-lg border border-dashed border-line-strong px-3 py-2 text-left hover:border-signal"
            >
              <span className="flex items-center gap-1 text-[13px]">
                <Glyph name="plus" size={11} /> {starter.label}
              </span>
              <span className="text-[11px] text-faint">{starter.hint}</span>
            </button>
          ))}
        </div>
        <div className="flex items-center gap-2">
          <Key tone="signal" size="sm" disabled={!dirty || saving} onClick={() => void save()}>
            {saving ? "Saving…" : "Save sources"}
          </Key>
          {dirty ? (
            <Key size="sm" tone="ghost" onClick={() => setDraft(saved ?? [])}>
              Undo changes
            </Key>
          ) : null}
        </div>
      </aside>

      <section>
        {current ? (
          <SourceEditor
            key={current.id}
            desktopId={desktopId}
            source={current}
            status={status[`src.${current.id}`]}
            secrets={secrets}
            onForget={(arg) => void api.forgetSourceSecret(desktopId, current.id, null, arg).then(load)}
            onChange={(change) => update(current.id, change)}
            onRemove={() => {
              setDraft((list) => list.filter((source) => source.id !== current.id));
              setSelected(null);
            }}
          />
        ) : (
          <div className="flex h-full min-h-60 items-center justify-center rounded-plate border border-dashed border-line text-[13px] text-muted">
            Pick a source, or start one from the left.
          </div>
        )}
      </section>
    </div>
  );
}

function ago(seconds: number): string {
  const delta = Math.max(0, Math.round(Date.now() / 1000 - seconds));
  if (delta < 60) return `${delta}s ago`;
  if (delta < 3600) return `${Math.round(delta / 60)}m ago`;
  return `${Math.round(delta / 3600)}h ago`;
}

function SourceEditor({
  desktopId,
  source,
  status,
  secrets,
  onForget,
  onChange,
  onRemove,
}: {
  desktopId: string;
  source: DataSource;
  status?: SourceStatus;
  secrets: readonly string[];
  onForget: (arg: string) => void;
  onChange: (change: (source: DataSource) => DataSource) => void;
  onRemove: () => void;
}) {
  const api = useApi();
  const [test, setTest] = useState<SourceTest | null>(null);
  const [testing, setTesting] = useState(false);
  const kind = source.kind ?? "http";
  const fields = source.fields ?? [];
  const args = source.args ?? [];

  const run = async () => {
    setTesting(true);
    try {
      setTest(await api.testSource(desktopId, source));
    } catch (problem) {
      setTest({ error: String(problem instanceof Error ? problem.message : problem) });
    } finally {
      setTesting(false);
    }
  };

  const addField = (path: (string | number)[], series: boolean) =>
    onChange((s) => ({
      ...s,
      fields: [...(s.fields ?? []), { name: fieldNameFor(path.join("."), (s.fields ?? []).map((f) => f.name)), value: pathExpression(path), ...(series ? { series: true } : {}) }],
    }));

  return (
    <div className="flex flex-col gap-5 rounded-plate border border-line bg-plate p-5">
      <div className="flex items-start gap-3">
        <div className="flex flex-1 flex-col gap-1">
          <DraftInput aria-label="Name" value={source.name ?? ""} placeholder="Name it" onCommit={(name) => onChange((s) => ({ ...s, name }))} className={cn(control, "h-10 text-lg font-semibold")} />
          <span className="font-mono text-[11px] text-faint">
            src.{source.id}.<span className="text-muted">field</span>
          </span>
        </div>
        <label className="flex items-center gap-2 pt-2 text-[12px] text-muted">
          On <Switch size="sm" label="Enabled" checked={source.enabled ?? true} onChange={(enabled) => onChange((s) => ({ ...s, enabled }))} />
        </label>
        <Key size="sm" tone="danger" onClick={onRemove}>
          Remove
        </Key>
      </div>

      {status && !status.ok && status.error ? <Notice tone="warn">{status.error}</Notice> : null}

      <Block title="What it reads">
        <Segmented
          size="sm"
          label="Kind"
          value={kind}
          onChange={(next) =>
            onChange((s) => ({
              ...s,
              kind: next,
              ...(next === "http" ? { http: s.http ?? { url: "https://" } } : { command: s.command ?? { program: "", args: [] } }),
            }))
          }
          options={[
            { value: "http", label: "A web address" },
            { value: "command", label: "A program" },
          ]}
        />
        {kind === "http" ? (
          <>
            <Field label="Address">
              <DraftInput aria-label="Address" value={source.http?.url ?? ""} placeholder="https://…" onCommit={(url) => onChange((s) => ({ ...s, http: { ...(s.http ?? { url: "" }), url } }))} className={cn(control, "h-9 flex-1 font-mono text-[12px]")} />
            </Field>
            <Field label="Method">
              <select aria-label="Method" value={source.http?.method || "GET"} onChange={(event) => onChange((s) => ({ ...s, http: { ...(s.http ?? { url: "" }), method: event.target.value === "GET" ? undefined : event.target.value } }))} className={cn(control, "h-9 w-28")}>
                {["GET", "POST", "PUT", "PATCH", "DELETE"].map((m) => (
                  <option key={m}>{m}</option>
                ))}
              </select>
            </Field>
            <Pairs
              label="Headers"
              pairs={source.http?.headers ?? {}}
              placeholder={["Authorization", "Bearer {{vars.token}}"]}
              onChange={(headers) => onChange((s) => ({ ...s, http: { ...(s.http ?? { url: "" }), headers } }))}
            />
            {source.http?.method && source.http.method !== "GET" ? (
              <Field label="Body">
                <DraftInput aria-label="Body" value={source.http?.body ?? ""} placeholder='{"entity_id": "light.desk"}' onCommit={(body) => onChange((s) => ({ ...s, http: { ...(s.http ?? { url: "" }), body } }))} className={cn(control, "h-9 flex-1 font-mono text-[12px]")} />
              </Field>
            ) : null}
            <label className="flex items-start gap-2 text-[12px] text-muted">
              <Switch size="sm" label="May reach my network" checked={source.lan ?? false} onChange={(lan) => onChange((s) => ({ ...s, lan: lan || undefined }))} />
              <span>
                May reach this computer and my network — for Home Assistant, a router, a NAS. Off, a source can only reach the internet.
              </span>
            </label>
          </>
        ) : (
          <>
            <Field label="Program">
              <DraftInput aria-label="Program" value={source.command?.program ?? ""} placeholder="powershell" onCommit={(program) => onChange((s) => ({ ...s, command: { ...(s.command ?? { program: "" }), program } }))} className={cn(control, "h-9 flex-1 font-mono text-[12px]")} />
            </Field>
            <Field label="Arguments">
              <DraftInput
                aria-label="Arguments"
                value={(source.command?.args ?? []).join(" | ")}
                placeholder="-NoProfile | -Command | (Get-Date).Hour"
                onCommit={(text) => onChange((s) => ({ ...s, command: { ...(s.command ?? { program: "" }), args: text.split("|").map((a) => a.trim()).filter(Boolean) } }))}
                className={cn(control, "h-9 flex-1 font-mono text-[12px]")}
              />
            </Field>
            <p className="m-0 text-[11px] text-faint">Run as written, without a shell; separate arguments with |. It must finish within 10 seconds.</p>
          </>
        )}
        <Field label="Every">
          <DraftInput
            aria-label="Every"
            value={String(source.everySeconds || (kind === "http" ? 300 : 60))}
            onCommit={(text) => onChange((s) => ({ ...s, everySeconds: Math.max(kind === "http" ? 30 : 5, Math.round(Number(text) || 0)) }))}
            className={cn(control, "h-9 w-24 text-right font-mono")}
          />
          <span className="text-[12px] text-muted">seconds, while something shows it</span>
        </Field>
      </Block>

      <Block
        title="Settings a key can change"
        hint="Placeholders like {{args.city}} in the address; each key can run its own copy with its own values. Mark a token or key secret: it stays on this computer, out of the deck and anything shared."
      >
        {args.map((arg, index) => {
          const setArg = (change: Partial<NonNullable<DataSource["args"]>[number]>) =>
            onChange((s) => ({ ...s, args: (s.args ?? []).map((a, at) => (at === index ? { ...a, ...change } : a)) }));
          const kept = arg.secret ? keptSecret(secrets, source.id, null, arg.name) : null;
          return (
            <div key={index} className="flex flex-wrap items-center gap-2">
              <DraftInput aria-label="Setting" value={arg.name} placeholder="city" onCommit={(name) => setArg({ name: name.replace(/[^A-Za-z0-9_]/g, "") })} className={cn(control, "h-8 w-32 font-mono")} />
              <span className="text-[12px] text-muted">=</span>
              <DraftInput
                aria-label="Default"
                type={arg.secret ? "password" : "text"}
                autoComplete="off"
                value={arg.default ?? ""}
                placeholder={arg.secret ? (kept ? "Kept on this computer — type to replace" : "Not set") : "Berlin"}
                onCommit={(value) => setArg({ default: value })}
                className={cn(control, "h-8 min-w-32 flex-1")}
              />
              <label className="flex items-center gap-1.5 text-[11px] text-muted" title="A token or key: kept on this computer only">
                <Switch size="sm" label="Secret" checked={arg.secret ?? false} onChange={(secret) => setArg({ secret: secret || undefined })} />
                Secret
              </label>
              {kept ? (
                <button type="button" onClick={() => onForget(arg.name)} className="text-[11px] text-muted hover:text-bad">
                  Forget
                </button>
              ) : null}
              <IconButton label="Remove setting" onClick={() => onChange((s) => ({ ...s, args: (s.args ?? []).filter((_, at) => at !== index) }))} />
            </div>
          );
        })}
        <AddButton onClick={() => onChange((s) => ({ ...s, args: [...(s.args ?? []), { name: `setting${args.length + 1}`, default: "" }] }))}>Add a setting</AddButton>
      </Block>

      <Block title="Fields" hint="Each becomes a value: src.id.name. Read the answer with json.a.b, text or status; filters work here too.">
        {fields.map((field, index) => (
          <div key={index} className="flex items-center gap-2">
            <DraftInput aria-label="Field name" value={field.name} onCommit={(name) => onChange((s) => ({ ...s, fields: (s.fields ?? []).map((f, at) => (at === index ? { ...f, name: name.replace(/[^A-Za-z0-9_]/g, "") } : f)) }))} className={cn(control, "h-8 w-32 font-mono")} />
            <span className="text-[12px] text-muted">=</span>
            <DraftInput aria-label="Reads" value={field.value} onCommit={(value) => onChange((s) => ({ ...s, fields: (s.fields ?? []).map((f, at) => (at === index ? { ...f, value } : f)) }))} className={cn(control, "h-8 flex-1 font-mono text-[12px]")} />
            <label className="flex items-center gap-1 text-[11px] text-muted" title="A list of numbers: drawn by a graph">
              List <Switch size="sm" label="List" checked={field.series ?? false} onChange={(series) => onChange((s) => ({ ...s, fields: (s.fields ?? []).map((f, at) => (at === index ? { ...f, series: series || undefined } : f)) }))} />
            </label>
            {test?.values && field.name in test.values ? <span className="max-w-32 truncate font-mono text-[11px] text-signal">{String(test.values[field.name] ?? "–")}</span> : null}
            <IconButton label="Remove field" onClick={() => onChange((s) => ({ ...s, fields: (s.fields ?? []).filter((_, at) => at !== index) }))} />
          </div>
        ))}
        <AddButton onClick={() => onChange((s) => ({ ...s, fields: [...fields, { name: `field${fields.length + 1}`, value: "text" }] }))}>Add a field</AddButton>
      </Block>

      <div className="flex flex-col gap-3">
        <div className="flex items-center gap-3">
          <Key size="sm" onClick={() => void run()} disabled={testing}>
            {testing ? "Testing…" : "Test it now"}
          </Key>
          <span className="text-[11px] text-faint">Runs it once with the settings' defaults. Nothing is saved.</span>
        </div>
        {test?.error ? <Notice tone="bad">{test.error}</Notice> : null}
        {test && !test.error ? (
          <div className="flex flex-col gap-2 rounded-xl border border-line bg-well p-3 shadow-well">
            <span className="lbl">Answered {test.status} — click a value to read it as a field</span>
            {test.json !== undefined && test.json !== null ? (
              <JsonTree value={test.json} path={[]} onPick={addField} />
            ) : (
              <pre className="m-0 max-h-60 overflow-auto font-mono text-[11px] whitespace-pre-wrap text-ink-2">{test.text}</pre>
            )}
          </div>
        ) : null}
      </div>

      {kind === "http" ? <ShareAsPlugin source={source} /> : null}
    </div>
  );
}

/**
 * Pack this source as a recipe: a plugin with no code, installable on any
 * desktop from the Plugins tab, with a gallery key that reads it. Its
 * secret settings go blank.
 */
function ShareAsPlugin({ source }: { source: DataSource }) {
  const [open, setOpen] = useState(false);
  const [meta, setMeta] = useState<RecipeMeta>(() => ({ id: recipeIdFor(source), name: source.name || source.id, version: "1.0.0", description: "", author: "" }));
  const problem = recipeProblem(source, meta);
  const save = () => {
    const bytes = packRecipe(source, meta);
    const link = document.createElement("a");
    link.href = URL.createObjectURL(new Blob([bytes as BlobPart], { type: "application/zip" }));
    link.download = `${meta.id}-${meta.version || "1.0.0"}.fdpkg`;
    link.click();
    setTimeout(() => URL.revokeObjectURL(link.href), 10_000);
  };
  const field = (key: keyof RecipeMeta, label: string, placeholder = "") => (
    <Field label={label}>
      <DraftInput aria-label={label} value={meta[key] ?? ""} placeholder={placeholder} onCommit={(value) => setMeta((m) => ({ ...m, [key]: value.trim() }))} className={cn(control, "h-8")} />
    </Field>
  );
  return (
    <Block title="Share as a plugin" hint="A file anyone can install from the Plugins tab: this source, and a key in the gallery that shows it. Secret settings go blank.">
      {open ? (
        <div className="flex flex-col gap-2">
          {field("name", "Name", "Bitcoin price")}
          {field("id", "Id", "bitcoin-price")}
          {field("version", "Version", "1.0.0")}
          {field("description", "What it does", "The price of a coin, every minute")}
          {field("author", "By", "Your name")}
          {problem ? <Notice tone="warn">{problem}</Notice> : null}
          <div className="flex gap-2">
            <Key size="sm" tone="signal" disabled={problem !== null} onClick={save}>
              Save the plugin file
            </Key>
            <Key size="sm" tone="ghost" onClick={() => setOpen(false)}>
              Cancel
            </Key>
          </div>
        </div>
      ) : (
        <Key size="sm" onClick={() => setOpen(true)}>
          Make it a plugin…
        </Key>
      )}
    </Block>
  );
}

/** An answer, as a tree: click a value to read it, a list of numbers to graph it. */
function JsonTree({ value, path, onPick, depth = 0 }: { value: unknown; path: (string | number)[]; onPick: (path: (string | number)[], series: boolean) => void; depth?: number }) {
  const kind = describeJson(value);
  if (kind === "object" || kind === "list") {
    const entries: [string | number, unknown][] = Array.isArray(value) ? value.slice(0, 20).map((item, index) => [index, item]) : Object.entries(value as Record<string, unknown>).slice(0, 60);
    if (depth > 6) return <span className="text-faint">…</span>;
    return (
      <ul className={cn("m-0 flex list-none flex-col gap-0.5 p-0 font-mono text-[12px]", depth > 0 && "ml-4 border-l border-line pl-2")}>
        {entries.map(([key, item]) => (
          <li key={String(key)}>
            <span className="text-muted">{key}:</span> <JsonTree value={item} path={[...path, key]} onPick={onPick} depth={depth + 1} />
          </li>
        ))}
      </ul>
    );
  }
  if (kind === "numbers") {
    const list = value as number[];
    return (
      <button type="button" onClick={() => onPick(path, true)} className="cursor-pointer rounded px-1 font-mono text-[12px] text-signal hover:bg-signal-soft" title="Read as a list, for a graph">
        [{list.slice(0, 6).join(", ")}
        {list.length > 6 ? `, … ${list.length}` : ""}]
      </button>
    );
  }
  return (
    <button type="button" onClick={() => onPick(path, false)} className="cursor-pointer rounded px-1 font-mono text-[12px] text-ink hover:bg-signal-soft hover:text-signal-hi" title={`Read ${pathExpression(path)} as a field`}>
      {kind === "text" ? JSON.stringify(value) : String(value)}
    </button>
  );
}

function Pairs({ label, pairs, placeholder, onChange }: { label: string; pairs: Record<string, string>; placeholder: [string, string]; onChange: (pairs: Record<string, string>) => void }) {
  const entries = Object.entries(pairs);
  const set = (list: [string, string][]) => onChange(Object.fromEntries(list.filter(([name]) => name.trim())));
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-[12px] text-muted">{label}</span>
      {entries.map(([name, value], index) => (
        <div key={index} className="flex items-center gap-2">
          <DraftInput aria-label={`${label} name`} value={name} onCommit={(next) => set(entries.map((e, at) => (at === index ? [next, e[1]] : e)))} className={cn(control, "h-8 w-40 font-mono text-[12px]")} />
          <DraftInput aria-label={`${label} value`} value={value} onCommit={(next) => set(entries.map((e, at) => (at === index ? [e[0], next] : e)))} className={cn(control, "h-8 flex-1 font-mono text-[12px]")} />
          <IconButton label={`Remove ${label.toLowerCase()}`} onClick={() => set(entries.filter((_, at) => at !== index))} />
        </div>
      ))}
      <AddButton onClick={() => set([...entries, [placeholder[0], placeholder[1]]])}>Add a {label.toLowerCase().replace(/s$/, "")}</AddButton>
    </div>
  );
}

function Block({ title, hint, children }: { title: string; hint?: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2.5">
      <div className="flex flex-col gap-0.5">
        <Label>{title}</Label>
        {hint ? <span className="text-[11px] text-faint">{hint}</span> : null}
      </div>
      {children}
    </section>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center gap-2">
      <span className="w-20 shrink-0 text-[12px] text-muted">{label}</span>
      {children}
    </div>
  );
}

function AddButton({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button type="button" onClick={onClick} className="flex items-center gap-1 self-start text-[12px] text-signal hover:underline">
      <Glyph name="plus" size={11} /> {children}
    </button>
  );
}

function IconButton({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button type="button" aria-label={label} title={label} onClick={onClick} className="flex size-6 shrink-0 cursor-pointer items-center justify-center rounded-md text-muted hover:bg-plate-2 hover:text-ink">
      <Glyph name="close" size={12} />
    </button>
  );
}
