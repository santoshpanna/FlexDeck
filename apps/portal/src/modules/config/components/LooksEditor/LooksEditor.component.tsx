import type { Face } from "@flexdeck/face";
import { faceFor } from "@flexdeck/face";
import { useEffect, useRef, useState } from "react";

import { Glyph, Label, Segmented, Switch, cn } from "@flexdeck/ui";

import type { CaseOp, Gesture, VariableChoice, WireCase, WireConfig, WireGestureLook, WireLook, WireRule, WireWidget } from "../../../../services/api/api.ts";
import {
  CASE_OPS,
  addLook,
  clearLookFace,
  editFace,
  lookSourceOf,
  nextLookId,
  removeLook,
  setLookRules,
  setLookSource,
  updateLook,
  type LookSource,
} from "../../lib/face.edit.lib.ts";
import { FACE_SWATCHES, INK_SWATCHES } from "../../lib/face.lib.ts";
import { HOLD_DOWN, HOLD_UP, SLOT_GESTURES, gestureBlocks, holdsOf } from "../../lib/flow.lib.ts";
import { ColorPicker } from "../FillPicker/ColorPicker.component.tsx";
import { FaceEditor } from "../FaceEditor/FaceEditor.component.tsx";
import { DraftInput, VariableInput, control } from "../Flow/fields.tsx";

type WidgetEdit = (transform: (config: WireConfig, widgetId: string) => WireConfig) => void;

/**
 * A key's states: several looks, and what picks one — each press, the last
 * gesture, or a live value ("above 80: Hot") — plus looks a value picks ahead
 * of that. Each look can change only the words and colours, or have a whole
 * face of its own.
 */
export function LooksEditor({
  widget,
  onEdit,
  variables,
  previewLook,
  onPreviewLook,
}: {
  widget: WireWidget;
  onEdit: WidgetEdit;
  variables: readonly VariableChoice[];
  /** The look the editor draws the key in; `""` for whichever applies. */
  previewLook: string;
  onPreviewLook: (lookId: string) => void;
}) {
  const looks = widget.states?.looks ?? [];
  const source = lookSourceOf(widget);
  const [open, setOpen] = useState<string | null>(null);
  const kind = source.kind === "none" ? "none" : source.kind;
  const choose = (next: LookSource) => onEdit((config, id) => setLookSource(config, id, next));
  const named = looks.map((look, index) => ({ id: look.id, name: lookName(look, index) }));
  const rules = widget.states?.rules ?? [];
  const gestures = gesturesOf(widget);

  return (
    <section className="flex flex-col gap-2.5">
      <div className="flex items-baseline justify-between gap-3">
        <Label>States</Label>
        <span className="text-[11px] text-faint">Other looks the key switches to</span>
      </div>

      <Segmented
        size="sm"
        label="What picks the state"
        value={kind}
        onChange={(next) =>
          choose(
            next === "cycle"
              ? { kind: "cycle" }
              : next === "variable"
                ? { kind: "variable", variable: source.kind === "variable" ? source.variable : "", cases: source.kind === "variable" ? source.cases : [], otherwise: source.kind === "variable" ? source.otherwise : "" }
                : next === "gesture"
                  ? { kind: "gesture", picks: source.kind === "gesture" ? source.picks : [] }
                  : { kind: "none" },
          )
        }
        options={[
          { value: "none", label: "None" },
          { value: "cycle", label: "Each press" },
          { value: "gesture", label: "Gestures" },
          { value: "variable", label: "A value" },
        ]}
      />

      {source.kind === "variable" ? (
        <CasesEditor source={source} looks={named} variables={variables} onChange={choose} />
      ) : null}
      {source.kind === "gesture" ? <GesturesEditor source={source} gestures={gestures} looks={named} onChange={choose} /> : null}
      <p className="m-0 text-[11px] text-faint">
        {source.kind === "cycle"
          ? "Each press shows the next state, after the press does its work. It counts presses; it does not know what the press changed."
          : source.kind === "variable"
            ? "A live value picks the state, so the key shows what is really true — muted, live, recording."
            : source.kind === "gesture"
              ? "What you last did on the key picks the state. Only gestures the key does something for are listed."
              : "The key keeps the look set above. Pick what switches it, then add the states it switches between."}
      </p>

      <div className="flex flex-col gap-1.5">
        {looks.map((look, index) => {
          const expanded = open === look.id;
          const own = look.face !== undefined;
          return (
            <div key={look.id} className={cn("rounded-xl border", expanded ? "border-line-strong bg-plate-2" : "border-line bg-plate")}>
              <div className="flex items-center gap-2 px-2.5 py-2">
                <button type="button" onClick={() => setOpen(expanded ? null : look.id)} aria-expanded={expanded} className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left">
                  <span
                    aria-hidden="true"
                    className="size-3.5 shrink-0 rounded-sm border border-line"
                    style={{ background: look.bgColor ?? look.face?.background?.color ?? "linear-gradient(180deg, var(--fd-key-top), var(--fd-key-bottom))" }}
                  />
                  <span className="flex min-w-0 flex-1 flex-col">
                    <span className="truncate text-[13px]">{lookName(look, index)}</span>
                    <span className="truncate text-[11px] text-faint">{shownWhen(source, rules, gestures, look.id, index)}</span>
                  </span>
                  {own ? <span className="lbl text-signal">own face</span> : null}
                </button>
                <button
                  type="button"
                  aria-pressed={previewLook === look.id}
                  title="Show the key in this state while editing"
                  onClick={() => onPreviewLook(previewLook === look.id ? "" : look.id)}
                  className={cn("lbl cursor-pointer rounded px-1.5 py-0.5", previewLook === look.id ? "bg-signal-soft text-signal-hi" : "text-faint hover:text-ink")}
                >
                  View
                </button>
                <button type="button" aria-label={`Remove ${lookName(look, index)}`} onClick={() => onEdit((config, id) => removeLook(config, id, look.id))} className="flex size-6 cursor-pointer items-center justify-center rounded-md text-muted hover:bg-plate-2 hover:text-ink">
                  <Glyph name="close" size={12} />
                </button>
              </div>
              {expanded ? (
                <div className="flex flex-col gap-3 border-t border-line px-2.5 pt-2.5 pb-3">
                  <div className="flex items-center justify-between">
                    <span className="text-[12px] text-muted">A whole face of its own</span>
                    <Switch
                      size="sm"
                      label="Own face"
                      checked={own}
                      onChange={(on) => onEdit((config, id) => (on ? editFace(config, id, look.id, (face: Face) => face) : clearLookFace(config, id, look.id)))}
                    />
                  </div>
                  {own ? (
                    <FaceEditor
                      face={look.face ?? faceFor(widget, look.id)}
                      onChange={(change) => onEdit((config, id) => editFace(config, id, look.id, change))}
                      variables={variables}
                      canvas={{ widget, lookId: look.id }}
                    />
                  ) : (
                    <>
                      <div className="flex items-center gap-2">
                        <span className="w-14 shrink-0 text-[12px] text-muted">Says</span>
                        <DraftInput aria-label="Says" value={look.label ?? ""} placeholder="the key's own words" onCommit={(label) => onEdit((config, id) => updateLook(config, id, look.id, { label }))} className={cn(control, "h-8 flex-1")} />
                      </div>
                      <div className="flex items-center gap-2">
                        <span className="w-14 shrink-0 text-[12px] text-muted">Icon</span>
                        <DraftInput aria-label="Icon" value={look.icon ?? ""} placeholder="the key's own icon" onCommit={(icon) => onEdit((config, id) => updateLook(config, id, look.id, { icon }))} className={cn(control, "h-8 flex-1 font-mono")} />
                      </div>
                      <ColorPicker label="Face" swatches={FACE_SWATCHES} value={look.bgColor} onChange={(bgColor) => onEdit((config, id) => updateLook(config, id, look.id, { bgColor }))} />
                      <ColorPicker label="Text" swatches={INK_SWATCHES} value={look.fgColor} onChange={(fgColor) => onEdit((config, id) => updateLook(config, id, look.id, { fgColor }))} />
                    </>
                  )}
                </div>
              ) : null}
            </div>
          );
        })}
      </div>

      {looks.length > 0 ? (
        <RulesEditor rules={rules} looks={named} variables={variables} onChange={(next) => onEdit((config, id) => setLookRules(config, id, next))} />
      ) : null}

      <button
        type="button"
        onClick={() => {
          const id = nextLookId(widget);
          // No label: the key keeps its own words until this state is given some.
          onEdit((config, widgetId) => addLook(config, widgetId, { id }));
          setOpen(id);
        }}
        className="flex cursor-pointer items-center justify-center gap-1 rounded-lg border border-dashed border-line-strong px-2 py-1.5 text-[12px] text-muted hover:border-signal hover:text-signal"
      >
        <Glyph name="plus" size={11} /> Add a state
      </button>
    </section>
  );
}

/** A state's name in the list: what it says, else the light's on/off, else its place. */
function lookName(look: WireLook, index: number): string {
  if (look.label) return look.label;
  if (look.id === "on") return "On";
  if (look.id === "off") return "Off";
  return `State ${index + 1}`;
}

const opLabel = (op: CaseOp | undefined) => CASE_OPS.find((each) => each.op === (op ?? "COMPARE_OP_EQ"))?.label ?? "is";

type GestureRow = { gesture: Gesture; label: string; empty: string };

/** The gestures a key can be told apart by: the ones it does something for. A hold key only holds. */
function gesturesOf(widget: WireWidget): GestureRow[] {
  if (holdsOf(widget)) {
    return [
      { gesture: HOLD_DOWN, label: "While held", empty: "no change" },
      { gesture: HOLD_UP, label: "When let go", empty: "back to before" },
    ];
  }
  const has = (gesture: Gesture) => gestureBlocks(widget, gesture).length > 0;
  return [
    { gesture: SLOT_GESTURES.tap, label: "After a tap", empty: "no change" },
    ...(has(SLOT_GESTURES.double) ? [{ gesture: SLOT_GESTURES.double, label: "After a double tap", empty: "no change" }] : []),
    ...(has(SLOT_GESTURES.long) ? [{ gesture: SLOT_GESTURES.long, label: "After a long press", empty: "no change" }] : []),
  ];
}

/** When a state shows, in words: what the person needs to read the list. */
function shownWhen(source: LookSource, rules: readonly WireRule[], gestures: readonly GestureRow[], lookId: string, index: number): string {
  const when = rules.filter((rule) => rule.state === lookId).map((rule) => `while ${rule.variable} ${opLabel(rule.op)} ${rule.value ?? ""}`.trim());
  if (source.kind === "cycle") when.push(index === 0 ? "at first" : `after ${index} ${index === 1 ? "press" : "presses"}`);
  if (source.kind === "gesture") {
    for (const pick of source.picks.filter((each) => each.state === lookId)) {
      const gesture = gestures.find((each) => each.gesture === pick.gesture);
      if (gesture) when.push(gesture.label.toLowerCase());
    }
  }
  if (source.kind === "variable") {
    const value = source.variable || "the value";
    for (const c of source.cases.filter((each) => (each.state ?? "") === lookId)) when.push(`while ${value} ${opLabel(c.op)} ${c.value ?? ""}`.trim());
    if (source.otherwise === lookId) when.push("otherwise");
  }
  return when.length ? `Shows ${when.join(", or ")}` : "Not shown yet — pick when above or below";
}

function lookSelect(label: string, looks: readonly { id: string; name: string }[], value: string, onPick: (id: string) => void, empty = "the key's own") {
  return (
    <select aria-label={label} value={value} onChange={(event) => onPick(event.target.value)} className={cn(control, "h-8 flex-1")}>
      <option value="">{empty}</option>
      {looks.map((look) => (
        <option key={look.id} value={look.id}>
          {look.name}
        </option>
      ))}
    </select>
  );
}

function GesturesEditor({
  source,
  gestures,
  looks,
  onChange,
}: {
  source: Extract<LookSource, { kind: "gesture" }>;
  gestures: readonly GestureRow[];
  looks: { id: string; name: string }[];
  onChange: (source: LookSource) => void;
}) {
  const pick = (gesture: Gesture, state: string) => {
    const others = source.picks.filter((each) => each.gesture !== gesture);
    const picks: WireGestureLook[] = state ? [...others, { gesture, state }] : others;
    onChange({ kind: "gesture", picks });
  };
  return (
    <div className="flex flex-col gap-2 rounded-xl border border-line bg-well/40 p-2.5">
      {gestures.map(({ gesture, label, empty }) => (
        <div key={gesture} className="flex items-center gap-1.5">
          <span className="w-32 shrink-0 text-[12px] text-muted">{label}</span>
          {lookSelect(label, looks, source.picks.find((each) => each.gesture === gesture)?.state ?? "", (state) => pick(gesture, state), empty)}
        </div>
      ))}
      <p className="m-0 text-[11px] text-faint">Give the key a double tap or long press under Does to pick a state with it too.</p>
    </div>
  );
}

/** Whether two rule lists say the same, as `setLookRules` would save them. */
function sameRules(a: readonly WireRule[], b: readonly WireRule[]): boolean {
  const plain = (rule: WireRule) => [rule.variable?.trim() ?? "", rule.op ?? "COMPARE_OP_EQ", rule.value ?? "", rule.state ?? ""].join(" ");
  return a.length === b.length && a.every((rule, at) => plain(rule) === plain(b[at]!));
}

function RulesEditor({
  rules,
  looks,
  variables,
  onChange,
}: {
  rules: readonly WireRule[];
  looks: { id: string; name: string }[];
  variables: readonly VariableChoice[];
  onChange: (rules: WireRule[]) => void;
}) {
  // Every row lives here, half-filled ones included, so a row neither vanishes
  // while its save is on the way nor is lost when a save is refused. The saved
  // rules replace them only when what is saved changes and is not what the rows
  // already say — an undo, another editor. Compared by value: `rules` is a new
  // array on every render while there are none.
  const nextKey = useRef(0);
  const keyed = (rule: WireRule) => ({ key: nextKey.current++, rule });
  const [rows, setRows] = useState(() => rules.map(keyed));
  const complete = (rule: WireRule) => Boolean(rule.variable?.trim() && rule.state);
  const saved = JSON.stringify(rules);
  const seen = useRef(saved);
  useEffect(() => {
    if (saved === seen.current) return;
    seen.current = saved;
    setRows((current) => (sameRules(current.map((row) => row.rule).filter(complete), rules) ? current : rules.map(keyed)));
    // `saved` stands for `rules`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [saved]);
  const update = (next: { key: number; rule: WireRule }[]) => {
    setRows(next);
    onChange(next.map((row) => row.rule));
  };
  const set = (index: number, change: Partial<WireRule>) => update(rows.map((row, at) => (at === index ? { ...row, rule: { ...row.rule, ...change } } : row)));
  const remove = (index: number) => update(rows.filter((_, at) => at !== index));
  return (
    <div className="flex flex-col gap-2">
      <span className="text-[12px] text-muted">Also show a state while a value holds — checked first</span>
      {rows.map(({ key, rule }, index) => (
        <div key={key} className="flex flex-col gap-1.5 rounded-xl border border-line bg-well/40 p-2">
          <VariableInput label="While" value={rule.variable ?? ""} variables={variables} onCommit={(variable) => set(index, { variable })} />
          <div className="flex items-center gap-1.5">
            <select aria-label="Compares" value={rule.op ?? "COMPARE_OP_EQ"} onChange={(event) => set(index, { op: event.target.value as CaseOp })} className={cn(control, "h-8 w-28")}>
              {CASE_OPS.map((op) => (
                <option key={op.op} value={op.op}>
                  {op.label}
                </option>
              ))}
            </select>
            <DraftInput aria-label="Value" value={rule.value ?? ""} placeholder="true" onCommit={(value) => set(index, { value })} className={cn(control, "h-8 w-20 font-mono")} />
            <span className="text-[12px] text-muted">→</span>
            {lookSelect("Shows", looks, rule.state ?? "", (state) => set(index, { state }), "pick a state")}
            <button type="button" aria-label="Remove" onClick={() => remove(index)} className="flex size-6 cursor-pointer items-center justify-center rounded-md text-muted hover:text-ink">
              <Glyph name="close" size={12} />
            </button>
          </div>
        </div>
      ))}
      <button type="button" onClick={() => setRows([...rows, keyed({ op: "COMPARE_OP_EQ", value: "true", state: looks[0]?.id ?? "" })])} className="self-start text-[12px] text-signal hover:underline">
        + while a value…
      </button>
    </div>
  );
}

function CasesEditor({
  source,
  looks,
  variables,
  onChange,
}: {
  source: Extract<LookSource, { kind: "variable" }>;
  looks: { id: string; name: string }[];
  variables: readonly VariableChoice[];
  onChange: (source: LookSource) => void;
}) {
  const set = (change: Partial<typeof source>) => onChange({ ...source, ...change });
  const setCase = (index: number, change: Partial<WireCase>) => set({ cases: source.cases.map((c, at) => (at === index ? { ...c, ...change } : c)) });
  return (
    <div className="flex flex-col gap-2 rounded-xl border border-line bg-well/40 p-2.5">
      <VariableInput label="Follows" value={source.variable} variables={variables} onCommit={(variable) => set({ variable })} />
      {source.cases.map((c, index) => (
        <div key={index} className="flex items-center gap-1.5">
          <span className="text-[12px] text-muted">If it</span>
          <select aria-label="Compares" value={c.op ?? "COMPARE_OP_EQ"} onChange={(event) => setCase(index, { op: event.target.value as CaseOp })} className={cn(control, "h-8 w-28")}>
            {CASE_OPS.map((op) => (
              <option key={op.op} value={op.op}>
                {op.label}
              </option>
            ))}
          </select>
          <DraftInput aria-label="Value" value={c.value ?? ""} placeholder="80" onCommit={(value) => setCase(index, { value })} className={cn(control, "h-8 w-20 font-mono")} />
          <span className="text-[12px] text-muted">→</span>
          {lookSelect("Shows", looks, c.state ?? "", (state) => setCase(index, { state }))}
          <button type="button" aria-label="Remove case" onClick={() => set({ cases: source.cases.filter((_, at) => at !== index) })} className="flex size-6 cursor-pointer items-center justify-center rounded-md text-muted hover:text-ink">
            <Glyph name="close" size={12} />
          </button>
        </div>
      ))}
      <div className="flex items-center gap-1.5">
        <span className="text-[12px] text-muted">Otherwise</span>
        {lookSelect("Otherwise shows", looks, source.otherwise, (otherwise) => set({ otherwise }))}
      </div>
      <button
        type="button"
        onClick={() => set({ cases: [...source.cases, { op: "COMPARE_OP_EQ", value: "", state: looks[0]?.id ?? "" }] })}
        className="self-start text-[12px] text-signal hover:underline"
      >
        + another case
      </button>
      <p className="m-0 text-[11px] text-faint">Cases are tried in order; the first that holds shows.</p>
    </div>
  );
}
