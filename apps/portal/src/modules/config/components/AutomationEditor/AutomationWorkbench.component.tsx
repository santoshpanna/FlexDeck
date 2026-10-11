import { AnimatePresence, motion } from "motion/react";
import { useState, type ReactNode } from "react";

import { Glyph, HoldToConfirm, Key, Label, Led, Segmented, Switch, cn, item, list, spring } from "@flexdeck/ui";
import { AsciiScene, automate } from "@flexdeck/ui/ascii";

import type {
  ActionDefinition,
  Automation,
  AutomationDocument,
  Block,
  PluginEventChoice,
  Run,
  Trace,
  Trigger,
  VariableChoice,
  WirePage,
} from "../../../../services/api/api.ts";
import type { LoadChoices } from "../../hooks/useArgumentChoices/useArgumentChoices.hook.ts";
import { labelOf } from "../../lib/config.lib.ts";
import { emptyBlock } from "../../lib/flow.lib.ts";
import { control, DraftInput, ValueChip, VariableInput } from "../Flow/fields.tsx";
import { FlowLane } from "../Flow/FlowLane.component.tsx";
import { FlowProvider } from "../Flow/FlowProvider.component.tsx";
import { describeTrigger } from "./describe.lib.ts";

export interface AutomationWorkbenchProps {
  document: AutomationDocument;
  selectedId: string | null;
  onSelect: (automationId: string | null) => void;
  onChange: (next: AutomationDocument) => void;
  saving: "idle" | "saving" | "saved" | "error";
  dirty: boolean;
  onRetrySave: () => void;
  events: PluginEventChoice[];
  variables: VariableChoice[];
  catalogue: ActionDefinition[];
  loadChoices?: LoadChoices;
  pages: WirePage[];
  onDryRun: (automationId: string) => Promise<void>;
  lastRun: Run | null;
}

type Kind = Trigger["on"];

const KINDS: readonly { value: Kind; label: string }[] = [
  { value: "pluginEvent", label: "Event" },
  { value: "widget", label: "Key" },
  { value: "variables", label: "Value" },
  { value: "schedule", label: "Time" },
  { value: "manual", label: "Manual" },
];

const TEMPLATES: readonly { title: string; make: () => Omit<Automation, "id"> }[] = [
  {
    title: "When a key is pressed → run a sequence",
    make: () => ({ name: "Key sequence", enabled: true, trigger: { on: "widget", widgetId: "" }, body: [] }),
  },
  {
    title: "Weekdays at 9:00 → set things up",
    make: () => ({ name: "Morning setup", enabled: true, trigger: { on: "schedule", at: "09:00", days: ["mon", "tue", "wed", "thu", "fri"] }, body: [] }),
  },
  {
    title: "When a value changes → react",
    make: () => ({ name: "React to a change", enabled: true, trigger: { on: "variables", names: [] }, body: [emptyBlock("if")] }),
  },
];

function makeId(): string {
  return `a${Math.random().toString(36).slice(2, 10)}`;
}

/**
 * Automations as a workbench: the list on the left, the one being edited in
 * the middle as WHEN → THEN on a rail, and what a dry run saw on the right.
 * Edits save themselves a moment after they stop.
 */
export function AutomationWorkbench(p: AutomationWorkbenchProps) {
  const automations = p.document.automations ?? [];
  const selected = automations.find((automation) => automation.id === p.selectedId) ?? null;
  const [query, setQuery] = useState("");
  const shown = automations.filter((automation) => `${automation.name} ${describeTrigger(automation.trigger)}`.toLowerCase().includes(query.trim().toLowerCase()));

  const put = (next: Automation[]) => p.onChange({ ...p.document, automations: next });
  const update = (next: Automation) => put(automations.map((automation) => (automation.id === next.id ? next : automation)));
  const create = (seed: Omit<Automation, "id">) => {
    const id = makeId();
    put([...automations, { id, ...seed }]);
    p.onSelect(id);
  };

  return (
    <div className="grid items-start gap-5 lg:grid-cols-[280px_minmax(0,1fr)] xl:grid-cols-[280px_minmax(0,1fr)_300px]">
      <aside aria-label="Automations" className="flex flex-col gap-4 rounded-plate border border-line bg-plate p-4">
        <div className="flex items-center justify-between">
          <Label>Automations · {automations.length}</Label>
          <Key tone="signal" size="sm" onClick={() => create({ name: "New automation", enabled: true, trigger: { on: "manual" }, body: [] })}>
            <Glyph name="plus" size={13} /> New
          </Key>
        </div>
        <input aria-label="Search automations" placeholder="Search" value={query} onChange={(event) => setQuery(event.target.value)} className={control} />
        <motion.ul variants={list} initial="initial" animate="animate" className="m-0 flex list-none flex-col gap-1 p-0">
          <AnimatePresence initial={false}>
            {shown.map((automation) => {
              const on = automation.id === p.selectedId;
              return (
                <motion.li key={automation.id} layout variants={item} initial="initial" animate="animate" exit="exit">
                  <div className={cn("flex items-center gap-3 rounded-[14px] border px-3 py-2.5 transition-colors", on ? "border-line-strong bg-plate-2" : "border-transparent hover:bg-plate-2/60")}>
                    <button type="button" onClick={() => p.onSelect(automation.id)} className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left">
                      <TriggerGlyph kind={automation.trigger.on} />
                      <span className="min-w-0">
                        <span className="block truncate text-sm">{automation.name || "Untitled"}</span>
                        <span className="block truncate font-mono text-[10px] tracking-[0.06em] text-muted uppercase">{describeTrigger(automation.trigger)}</span>
                      </span>
                    </button>
                    <Switch size="sm" label={`Turn ${automation.name} ${automation.enabled === false ? "on" : "off"}`} checked={automation.enabled !== false} onChange={(enabled) => update({ ...automation, enabled })} />
                  </div>
                </motion.li>
              );
            })}
          </AnimatePresence>
        </motion.ul>
        {automations.length === 0 ? <p className="m-0 text-[13px] text-muted">None yet. Automations let the desktop react on its own.</p> : null}
        <div className="flex flex-col gap-2 border-t border-dashed border-line pt-4">
          <Label>Start from a template</Label>
          {TEMPLATES.map((template) => (
            <button
              key={template.title}
              type="button"
              onClick={() => create(template.make())}
              className="cursor-pointer rounded-xl border border-line bg-body px-3 py-2.5 text-left text-[13px] transition-colors hover:border-line-strong hover:bg-plate-2"
            >
              {template.title}
            </button>
          ))}
        </div>
      </aside>

      <AnimatePresence mode="wait" initial={false}>
        {selected ? (
          <motion.section
            key={selected.id}
            initial={{ opacity: 0, x: 12 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -8 }}
            transition={{ duration: 0.24 }}
            aria-label="Flow"
            className="flex min-w-0 flex-col gap-5"
          >
            <Editor {...p} automation={selected} onUpdate={update} onDelete={() => {
              put(automations.filter((automation) => automation.id !== selected.id));
              p.onSelect(null);
            }} />
          </motion.section>
        ) : (
          <motion.section key="empty" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="flex min-h-[420px] flex-col items-center justify-center gap-4 rounded-plate border border-dashed border-line-strong p-8 text-center">
            <AsciiScene draw={(t, cols) => automate(t, cols, 13)} fit={{ min: 52, max: 110 }} fps={24} size={10} tone="text-muted" still={4} className="max-w-3xl" />
            <h2 className="m-0 text-2xl font-semibold">When something happens, do something.</h2>
            <p className="m-0 max-w-md text-sm text-muted">Pick an automation on the left, or start one from a template. Each is a trigger and a list of steps, saved as you go.</p>
          </motion.section>
        )}
      </AnimatePresence>

      <RunPanel run={p.lastRun} selected={selected} onDryRun={p.onDryRun} />
    </div>
  );
}

function Editor({
  automation,
  onUpdate,
  onDelete,
  saving,
  dirty,
  onRetrySave,
  events,
  variables,
  catalogue,
  loadChoices,
  pages,
  onDryRun,
}: AutomationWorkbenchProps & { automation: Automation; onUpdate: (next: Automation) => void; onDelete: () => void }) {
  const setBody = (body: Block[]) => onUpdate({ ...automation, body });
  const state = saving === "error" ? "error" : saving === "saving" || dirty ? "saving" : "saved";
  return (
    <FlowProvider catalogue={catalogue} variables={variables} loadChoices={loadChoices} onAppend={(block) => setBody([...automation.body, block])}>
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <Label>Automation</Label>
          <DraftInput
            aria-label="Name"
            value={automation.name}
            onCommit={(name) => onUpdate({ ...automation, name })}
            className="h-12 border-transparent bg-transparent px-0 text-3xl font-semibold tracking-[-0.01em] shadow-none hover:border-line focus:px-3"
          />
        </div>
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={state === "error" ? onRetrySave : undefined}
            className={cn("flex items-center gap-2 font-mono text-[10px] tracking-[0.18em] uppercase", state === "error" ? "cursor-pointer text-bad" : "cursor-default text-muted")}
          >
            <Led state={state === "error" ? "bad" : state === "saving" ? "wait" : "ok"} />
            {state === "error" ? "Not saved · retry" : state === "saving" ? "Saving" : "Saved"}
          </button>
          <Key tone="signal" size="md" onClick={() => void onDryRun(automation.id)}>
            <svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path d="M6 4l14 8-14 8z" />
            </svg>
            Dry run
          </Key>
          <HoldToConfirm size="md" tone="danger" onConfirm={onDelete}>
            Delete
          </HoldToConfirm>
        </div>
      </div>

      <section className="flex flex-col gap-3 rounded-plate border border-signal-line/60 bg-plate p-4">
        <div className="flex flex-wrap items-center gap-3">
          <span className="inline-flex h-7 items-center rounded-[7px] border border-signal-line bg-body px-2.5 font-mono text-[10px] font-bold tracking-[0.18em] text-signal">WHEN</span>
          <Segmented size="sm" label="Trigger" value={automation.trigger.on} onChange={(kind) => onUpdate({ ...automation, trigger: blankTrigger(kind) })} options={KINDS} />
        </div>
        <TriggerFields trigger={automation.trigger} onChange={(trigger) => onUpdate({ ...automation, trigger })} events={events} variables={variables} pages={pages} />
      </section>

      <section className="flex flex-col gap-3">
        <div className="flex items-center gap-3">
          <span className="inline-flex h-7 items-center rounded-[7px] border border-line bg-body px-2.5 font-mono text-[10px] font-bold tracking-[0.18em] text-ink">THEN</span>
          <span className="text-[12px] text-faint">Steps run top to bottom.</span>
        </div>
        <FlowLane blocks={automation.body} onChange={setBody} empty="Add the first step" />
      </section>
    </FlowProvider>
  );
}

function blankTrigger(kind: Kind): Trigger {
  switch (kind) {
    case "variables":
      return { on: "variables", names: [] };
    case "widget":
      return { on: "widget", widgetId: "" };
    case "pluginEvent":
      return { on: "pluginEvent", plugin: "", event: "", filter: {} };
    case "schedule":
      return { on: "schedule", everySeconds: 300 };
    case "manual":
      return { on: "manual" };
  }
}

const DAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] as const;

function TriggerFields({
  trigger,
  onChange,
  events,
  variables,
  pages,
}: {
  trigger: Trigger;
  onChange: (next: Trigger) => void;
  events: PluginEventChoice[];
  variables: VariableChoice[];
  pages: WirePage[];
}) {
  const sentence = "flex flex-wrap items-center gap-2 text-[15px]";
  switch (trigger.on) {
    case "manual":
      return <p className="m-0 text-sm text-muted">Only when a key or another automation runs it.</p>;
    case "variables":
      return (
        <div className={sentence}>
          {trigger.names.map((name) => (
            <span key={name} className="inline-flex items-center gap-1">
              <ValueChip name={name} />
              <button type="button" aria-label={`Stop watching ${name}`} onClick={() => onChange({ on: "variables", names: trigger.names.filter((other) => other !== name) })} className="cursor-pointer text-faint hover:text-bad">
                ×
              </button>
            </span>
          ))}
          <span className="text-muted">{trigger.names.length === 0 ? "any value" : ""} changes</span>
          <VariableInput
            label="Watch another value"
            value=""
            variables={variables}
            onCommit={(name) => name && !trigger.names.includes(name) && onChange({ on: "variables", names: [...trigger.names, name] })}
            className="w-48"
          />
        </div>
      );
    case "widget":
      return <KeyPicker pages={pages} value={trigger.widgetId} onChange={(widgetId) => onChange({ on: "widget", widgetId })} />;
    case "pluginEvent": {
      const chosen = events.find((event) => event.plugin === trigger.plugin && event.event === trigger.event);
      const filter = trigger.filter ?? {};
      return (
        <div className="flex flex-col gap-3">
          <div className={sentence}>
            <select
              aria-label="Event"
              className={cn(control, "w-auto min-w-[260px]")}
              value={chosen ? `${chosen.plugin}:${chosen.event}` : ""}
              onChange={(event) => {
                const [plugin = "", name = ""] = event.target.value.split(":");
                onChange({ on: "pluginEvent", plugin, event: name, filter: {} });
              }}
            >
              <option value="">Choose what happens…</option>
              {events.map((event) => (
                <option key={`${event.plugin}:${event.event}`} value={`${event.plugin}:${event.event}`}>
                  {event.pluginName} — {event.description || event.event}
                </option>
              ))}
            </select>
          </div>
          {chosen && chosen.fields.length > 0 ? (
            <div className="flex flex-col gap-2">
              {chosen.fields.map((field) => (
                <label key={field} className={sentence}>
                  <span className="text-muted">only when</span>
                  <ValueChip name={field} />
                  <span className="text-muted">is</span>
                  <DraftInput
                    aria-label={`${field} must be`}
                    value={filter[field] ?? ""}
                    placeholder="anything"
                    onCommit={(value) => {
                      const next = { ...filter };
                      if (value === "") delete next[field];
                      else next[field] = value;
                      onChange({ ...trigger, filter: next });
                    }}
                    className="w-48"
                  />
                </label>
              ))}
            </div>
          ) : null}
        </div>
      );
    }
    case "schedule": {
      const daily = trigger.at !== undefined;
      return (
        <div className="flex flex-col gap-3">
          <Segmented
            size="sm"
            label="Schedule"
            value={daily ? "at" : "every"}
            onChange={(mode) => onChange(mode === "at" ? { on: "schedule", at: "09:00", days: [] } : { on: "schedule", everySeconds: 300 })}
            options={[
              { value: "every", label: "Every" },
              { value: "at", label: "At a time" },
            ]}
            className="self-start"
          />
          {daily ? (
            <div className={sentence}>
              <span className="text-muted">at</span>
              <input
                type="time"
                aria-label="Time"
                value={trigger.at ?? "09:00"}
                onChange={(event) => onChange({ on: "schedule", at: event.target.value, days: trigger.days ?? [] })}
                className={cn(control, "w-auto font-mono")}
              />
              <span className="text-muted">on</span>
              <div className="flex flex-wrap gap-1" role="group" aria-label="Days">
                {DAYS.map((day) => {
                  const on = (trigger.days ?? []).includes(day);
                  return (
                    <button
                      key={day}
                      type="button"
                      aria-pressed={on}
                      onClick={() => {
                        const days = on ? (trigger.days ?? []).filter((other) => other !== day) : [...(trigger.days ?? []), day];
                        onChange({ on: "schedule", at: trigger.at, days });
                      }}
                      className={cn(
                        "h-8 w-11 cursor-pointer rounded-lg border font-mono text-[10px] tracking-[0.12em] uppercase transition-colors",
                        on ? "border-ink bg-ink text-body" : "border-line text-muted hover:text-ink",
                      )}
                    >
                      {day}
                    </button>
                  );
                })}
              </div>
              {(trigger.days ?? []).length === 0 ? <span className="text-[12px] text-faint">none picked = every day</span> : null}
            </div>
          ) : (
            <div className={sentence}>
              <span className="text-muted">every</span>
              <DraftInput
                aria-label="Minutes"
                value={String(Math.max(1, Math.round((trigger.everySeconds ?? 300) / 60)))}
                onCommit={(text) => onChange({ on: "schedule", everySeconds: Math.max(1, Math.round(Number(text) || 1)) * 60 })}
                className="w-20 text-center font-mono"
              />
              <span className="text-muted">minutes</span>
            </div>
          )}
        </div>
      );
    }
  }
}

/** Pick the key by pointing at it on a small map of each page, not by typing its id. */
function KeyPicker({ pages, value, onChange }: { pages: WirePage[]; value: string; onChange: (widgetId: string) => void }) {
  const [page, setPage] = useState(() => Math.max(0, pages.findIndex((candidate) => (candidate.widgets ?? []).some((widget) => widget.id === value))));
  const current = pages[page];
  const widgets = current?.widgets ?? [];
  const cols = Math.max(1, ...widgets.map((widget) => (widget.col ?? 0) + Math.max(1, widget.w ?? 1)));
  const chosen = pages.flatMap((candidate) => candidate.widgets ?? []).find((widget) => widget.id === value);
  return (
    <div className="flex flex-wrap items-start gap-5">
      <div className="flex flex-col gap-2">
        {pages.length > 1 ? (
          <div className="flex gap-1">
            {pages.map((candidate, index) => (
              <button
                key={candidate.id}
                type="button"
                onClick={() => setPage(index)}
                className={cn("h-6 cursor-pointer rounded-md px-2 font-mono text-[10px]", index === page ? "bg-ink text-body" : "text-muted hover:text-ink")}
              >
                {candidate.name || `Page ${index + 1}`}
              </button>
            ))}
          </div>
        ) : null}
        <div
          className="grid gap-1 rounded-xl border border-line bg-well p-2 shadow-well"
          style={{ gridTemplateColumns: `repeat(${cols}, 34px)`, gridAutoRows: "26px" }}
          role="group"
          aria-label="Keys"
        >
          {widgets.map((widget) => (
            <button
              key={widget.id}
              type="button"
              title={labelOf(widget)}
              aria-label={labelOf(widget)}
              aria-pressed={widget.id === value}
              onClick={() => onChange(widget.id)}
              style={{ gridColumn: `${(widget.col ?? 0) + 1} / span ${Math.max(1, widget.w ?? 1)}`, gridRow: `${(widget.row ?? 0) + 1} / span ${Math.max(1, widget.h ?? 1)}` }}
              className={cn(
                "cursor-pointer rounded-[5px] border transition-all",
                widget.id === value ? "scale-105 border-signal-hi bg-signal shadow-[0_0_10px_var(--color-signal)]" : "border-[var(--fd-key-line)] bg-plate-2 hover:border-line-strong",
              )}
            />
          ))}
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-2 pt-1 text-[15px]">
        <span className="text-muted">the key</span>
        {chosen ? <span className="rounded-lg border border-line bg-body px-2.5 py-1 font-mono text-[12px]">{labelOf(chosen)}</span> : <span className="text-warn">— pick one on the map</span>}
        <span className="text-muted">is pressed</span>
      </div>
    </div>
  );
}

function TriggerGlyph({ kind }: { kind: Kind }) {
  const glyph = kind === "schedule" ? "refresh" : kind === "widget" ? "tablet" : kind === "pluginEvent" ? "bolt" : kind === "variables" ? "sliders" : "power";
  return (
    <span className="flex size-9 shrink-0 items-center justify-center rounded-[10px] border border-line bg-body text-ink-2">
      <Glyph name={glyph} size={15} />
    </span>
  );
}

/** What a dry run saw, one line at a time, in the order it happened. Nothing in it reached an app. */
function RunPanel({ run, selected, onDryRun }: { run: Run | null; selected: Automation | null; onDryRun: (id: string) => Promise<void> }) {
  return (
    <aside aria-label="Dry run" className="flex flex-col gap-4 rounded-plate border border-line bg-plate p-4 lg:col-span-2 xl:sticky xl:top-[76px] xl:col-span-1">
      <div className="flex items-center justify-between">
        <Label>Dry run</Label>
        {run ? (
          <span className={cn("flex items-center gap-2 font-mono text-[10px] tracking-[0.16em] uppercase", run.outcome === "completed" ? "text-ok" : "text-warn")}>
            <Led state={run.outcome === "completed" ? "ok" : "warn"} /> {run.outcome}
          </span>
        ) : null}
      </div>
      <div className="flex min-h-[220px] flex-col gap-1.5 rounded-xl border border-line bg-well p-3 shadow-well">
        {run === null ? (
          <div className="flex flex-col gap-3">
            <p className="m-0 font-mono text-[11px] leading-relaxed text-faint">
              Walks the flow against a scratch copy of your values. Nothing reaches your apps.
            </p>
            {selected ? (
              <Key size="sm" className="self-start" onClick={() => void onDryRun(selected.id)}>
                Run it now
              </Key>
            ) : null}
          </div>
        ) : run.trace.length === 0 ? (
          <p className="m-0 font-mono text-[11px] text-faint">Nothing ran.</p>
        ) : (
          <motion.ol key={run.trace.length + run.outcome} initial="initial" animate="animate" variants={{ animate: { transition: { staggerChildren: 0.12 } } }} className="m-0 flex list-none flex-col gap-1.5 p-0">
            {run.trace.map((entry, index) => (
              <motion.li
                key={index}
                variants={{ initial: { opacity: 0, x: -6 }, animate: { opacity: 1, x: 0, transition: spring.panel } }}
                className="grid grid-cols-[22px_minmax(0,1fr)] gap-2 font-mono text-[11px] leading-relaxed"
              >
                <span className="text-faint">{String(index + 1).padStart(2, "0")}</span>
                <TraceLine entry={entry} />
              </motion.li>
            ))}
          </motion.ol>
        )}
      </div>
    </aside>
  );
}

function TraceLine({ entry }: { entry: Trace }): ReactNode {
  switch (entry.kind) {
    case "condition":
      return (
        <span className={entry.outcome.kind === "true" ? "text-ok" : entry.outcome.kind === "false" ? "text-warn" : "text-bad"}>
          {entry.summary} → {entry.outcome.kind === "unknown" ? `unknown (${entry.outcome.reason})` : entry.outcome.kind}
        </span>
      );
    case "waited":
      return <span className="text-ink-2">waited {entry.ms} ms</span>;
    case "repeated":
      return <span className="text-ink-2">repeated {entry.times}×</span>;
    case "set":
      return (
        <span className="text-ink-2">
          set {entry.name} = {"variable" in entry.value ? `{{${entry.value.variable}}}` : JSON.stringify(Object.values(entry.value)[0])}
        </span>
      );
    case "ran":
      return (
        <span className="text-ok">
          ✓ {entry.plugin}:{entry.action}
          {entry.input ? ` ${entry.input}` : ""}
        </span>
      );
    case "stopped":
      return <span className="text-bad">stopped: {entry.reason}</span>;
  }
}

