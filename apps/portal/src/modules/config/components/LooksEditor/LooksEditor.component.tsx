import type { Face } from "@flexdeck/face";
import { faceFor } from "@flexdeck/face";
import { useState } from "react";

import { Glyph, Label, Segmented, Switch, cn } from "@flexdeck/ui";

import type { CaseOp, VariableChoice, WireCase, WireConfig, WireWidget } from "../../../../services/api/api.ts";
import {
  CASE_OPS,
  addLook,
  clearLookFace,
  editFace,
  lookSourceOf,
  nextLookId,
  removeLook,
  setLookSource,
  updateLook,
  type LookSource,
} from "../../lib/face.edit.lib.ts";
import { FACE_SWATCHES, INK_SWATCHES } from "../../lib/face.lib.ts";
import { ColorPicker } from "../FillPicker/ColorPicker.component.tsx";
import { FaceEditor } from "../FaceEditor/FaceEditor.component.tsx";
import { DraftInput, VariableInput, control } from "../Flow/fields.tsx";

type WidgetEdit = (transform: (config: WireConfig, widgetId: string) => WireConfig) => void;

/**
 * A key's states: several looks, and what picks one — each press, or a live
 * value ("above 80: Hot"). Each look can change only the words and colours,
 * or have a whole face of its own.
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

  return (
    <section className="flex flex-col gap-2.5">
      <div className="flex items-baseline justify-between gap-3">
        <Label>States</Label>
        <span className="text-[11px] text-faint">Different words, colours or a whole face</span>
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
                : { kind: "none" },
          )
        }
        options={[
          { value: "none", label: "Nothing" },
          { value: "cycle", label: "Each press" },
          { value: "variable", label: "A value" },
        ]}
      />

      {source.kind === "variable" ? (
        <CasesEditor source={source} looks={looks.map((look) => ({ id: look.id, name: look.label || look.id }))} variables={variables} onChange={choose} />
      ) : null}
      {source.kind === "cycle" ? <p className="m-0 text-[11px] text-faint">Each press shows the next state, after the press does its work.</p> : null}

      <div className="flex flex-col gap-1.5">
        {looks.map((look) => {
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
                  <span className="min-w-0 flex-1 truncate text-[13px]">{look.label || look.id}</span>
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
                <button type="button" aria-label={`Remove ${look.label || look.id}`} onClick={() => onEdit((config, id) => removeLook(config, id, look.id))} className="flex size-6 cursor-pointer items-center justify-center rounded-md text-muted hover:bg-plate-2 hover:text-ink">
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

      <button
        type="button"
        onClick={() => {
          const id = nextLookId(widget);
          onEdit((config, widgetId) => addLook(config, widgetId, { id, label: `State ${looks.length + 1}` }));
          setOpen(id);
        }}
        className="flex cursor-pointer items-center justify-center gap-1 rounded-lg border border-dashed border-line-strong px-2 py-1.5 text-[12px] text-muted hover:border-signal hover:text-signal"
      >
        <Glyph name="plus" size={11} /> Add a state
      </button>
    </section>
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
  const lookSelect = (label: string, value: string, onPick: (id: string) => void) => (
    <select aria-label={label} value={value} onChange={(event) => onPick(event.target.value)} className={cn(control, "h-8 flex-1")}>
      <option value="">the key's own</option>
      {looks.map((look) => (
        <option key={look.id} value={look.id}>
          {look.name}
        </option>
      ))}
    </select>
  );
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
          {lookSelect("Shows", c.state ?? "", (state) => setCase(index, { state }))}
          <button type="button" aria-label="Remove case" onClick={() => set({ cases: source.cases.filter((_, at) => at !== index) })} className="flex size-6 cursor-pointer items-center justify-center rounded-md text-muted hover:text-ink">
            <Glyph name="close" size={12} />
          </button>
        </div>
      ))}
      <div className="flex items-center gap-1.5">
        <span className="text-[12px] text-muted">Otherwise</span>
        {lookSelect("Otherwise shows", source.otherwise, (otherwise) => set({ otherwise }))}
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
