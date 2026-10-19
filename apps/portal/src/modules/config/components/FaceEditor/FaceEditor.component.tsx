import { LAYOUTS, LAYOUT_SLOTS, boxOf, placeFace, type Background, type ElementKind, type Face, type FaceElement, type FaceLayout, type Rule, type Slot } from "@flexdeck/face";
import { useState, type ReactNode } from "react";

import { Glyph, Label, Notice, Segmented, Switch, cn } from "@flexdeck/ui";

import type { VariableChoice, WidgetPreview, WireWidget } from "../../../../services/api/api.ts";
import { canvasGeometry } from "../../lib/canvas.lib.ts";
import { FaceCanvas } from "./FaceCanvas.component.tsx";
import {
  addElement,
  moveElement,
  removeElement,
  setBackground,
  setLayout,
  starterElement,
  updateElement,
} from "../../lib/face.edit.lib.ts";
import { FACE_SWATCHES, INK_SWATCHES } from "../../lib/face.lib.ts";
import { fillOf, toBackground } from "../../lib/fill.lib.ts";
import { ColorPicker } from "../FillPicker/ColorPicker.component.tsx";
import { FillPicker } from "../FillPicker/FillPicker.component.tsx";
import { IconPicker } from "../IconPicker/IconPicker.component.tsx";

import { DraftInput, ExpressionInput, ValueInput, VariableInput, control } from "../Flow/fields.tsx";

type FaceChange = (change: (face: Face) => Face) => void;

const LAYOUT_NAMES: Record<FaceLayout, { name: string; hint: string }> = {
  FACE_LAYOUT_ICON_LABEL: { name: "Icon + label", hint: "An icon over its name" },
  FACE_LAYOUT_VALUE: { name: "Value", hint: "A big number, a line above and below" },
  FACE_LAYOUT_ART: { name: "Picture", hint: "A picture filling the key, a title on it" },
  FACE_LAYOUT_SPLIT: { name: "Split", hint: "A visual beside its text" },
  FACE_LAYOUT_DIAL: { name: "Dial", hint: "A gauge or a clock, its value inside" },
};

const SLOT_NAMES: Record<Slot, string> = {
  SLOT_TITLE: "Title",
  SLOT_ICON: "Icon place",
  SLOT_VALUE: "Value",
  SLOT_CAPTION: "Caption",
  SLOT_TOP: "Top line",
  SLOT_BADGE: "Badge",
  SLOT_VISUAL: "Visual",
};

const KINDS: readonly { kind: ElementKind; name: string; hint: string }[] = [
  { kind: "ELEMENT_KIND_TEXT", name: "Text", hint: "Words or a live value" },
  { kind: "ELEMENT_KIND_ICON", name: "Icon", hint: "A bundled icon, or a picture" },
  { kind: "ELEMENT_KIND_BAR", name: "Bar", hint: "How full: a level, a progress" },
  { kind: "ELEMENT_KIND_GAUGE", name: "Gauge", hint: "A ring that fills" },
  { kind: "ELEMENT_KIND_GRAPH", name: "Graph", hint: "A value over the last minutes" },
  { kind: "ELEMENT_KIND_CLOCK", name: "Clock", hint: "The time, digital or with hands" },
  { kind: "ELEMENT_KIND_TIMER", name: "Timer", hint: "A countdown: a pomodoro, a tea" },
];

const kindName = (kind: ElementKind | undefined) => KINDS.find((k) => k.kind === (kind ?? "ELEMENT_KIND_TEXT"))?.name ?? "Text";

/**
 * A face, part by part: its layout, its elements in their places, its
 * background, and the rules that recolour it. Every change is one edit, so
 * undo steps back through them one at a time.
 */
/** The key being edited, for the canvas pieces are dragged about on. */
export interface FaceCanvasSource {
  widget: WireWidget;
  lookId?: string;
  preview?: WidgetPreview;
}

export function FaceEditor({
  face,
  onChange,
  variables,
  error,
  canvas,
  fill = true,
}: {
  face: Face;
  onChange: FaceChange;
  variables: readonly VariableChoice[];
  error?: string;
  canvas?: FaceCanvasSource;
  /** Whether it offers the background's fill: off where the key's fill is chosen above it. */
  fill?: boolean;
}) {
  const layout = face.layout ?? "FACE_LAYOUT_ICON_LABEL";
  const elements = face.elements ?? [];
  const [open, setOpen] = useState<number | null>(null);

  return (
    <div className="flex flex-col gap-5">
      {error ? <Notice tone="warn">{error}</Notice> : null}

      <section className="flex flex-col gap-2.5">
        <Label>Layout</Label>
        <div role="radiogroup" aria-label="Layout" className="grid grid-cols-5 gap-1.5">
          {LAYOUTS.map((candidate) => (
            <button
              key={candidate}
              type="button"
              role="radio"
              aria-checked={candidate === layout}
              title={LAYOUT_NAMES[candidate].hint}
              onClick={() => onChange((f) => setLayout(f, candidate))}
              className={cn(
                "flex cursor-pointer flex-col items-center gap-1 rounded-lg border p-1.5 transition-colors",
                candidate === layout ? "border-signal bg-signal-soft/50 text-signal-hi" : "border-line bg-plate text-muted hover:border-line-strong",
              )}
            >
              <LayoutThumb layout={candidate} />
              <span className="text-center text-[10px] leading-tight">{LAYOUT_NAMES[candidate].name}</span>
            </button>
          ))}
        </div>
      </section>

      {canvas && elements.length > 0 ? (
        <section className="flex flex-col gap-2.5">
          <Label>Arrange</Label>
          <FaceCanvas
            widget={canvas.widget}
            face={face}
            lookId={canvas.lookId}
            preview={canvas.preview}
            selected={open}
            onSelect={setOpen}
            onBox={(index, box) => onChange((f) => updateElement(f, index, compactElement({ ...(f.elements ?? [])[index]!, box })))}
          />
        </section>
      ) : null}

      <section className="flex flex-col gap-2.5">
        <Label>Elements</Label>
        {elements.length === 0 ? <p className="m-0 text-[12px] text-faint">Nothing on the key yet. Add something below.</p> : null}
        <div className="flex flex-col gap-1.5">
          {elements.map((element, index) => (
            <ElementCard
              key={index}
              index={index}
              count={elements.length}
              element={element}
              layout={layout}
              open={open === index}
              onToggle={() => setOpen(open === index ? null : index)}
              onChange={(next) => onChange((f) => updateElement(f, index, next))}
              onMove={(delta) => {
                onChange((f) => moveElement(f, index, delta));
                setOpen(open === index ? index + delta : open);
              }}
              onRemove={() => {
                onChange((f) => removeElement(f, index));
                setOpen(null);
              }}
              onFree={
                canvas
                  ? () => {
                      const { w, faceH, rects } = canvasGeometry(canvas.widget, face);
                      const box = boxOf(rects[index]!, w, faceH);
                      onChange((f) => updateElement(f, index, compactElement({ ...(f.elements ?? [])[index]!, box })));
                    }
                  : undefined
              }
              variables={variables}
            />
          ))}
        </div>
        <div className="grid grid-cols-3 gap-1.5">
          {KINDS.map((k) => (
            <button
              key={k.kind}
              type="button"
              title={k.hint}
              onClick={() => {
                onChange((f) => addElement(f, starterElement(k.kind, layout)));
                setOpen(elements.length);
              }}
              className="flex cursor-pointer items-center justify-center gap-1 rounded-lg border border-dashed border-line-strong px-2 py-1.5 text-[12px] text-muted hover:border-signal hover:text-signal"
            >
              <Glyph name="plus" size={11} /> {k.name}
            </button>
          ))}
        </div>
      </section>

      <BackgroundEditor background={face.background ?? {}} onChange={(background) => onChange((f) => setBackground(f, background))} variables={variables} fill={fill} />

      <section className="flex flex-col gap-2.5">
        <Label>Background rules</Label>
        <RulesEditor
          rules={face.rules ?? []}
          onChange={(rules) => onChange((f) => withRules(f, rules))}
          variables={variables}
          what="the background"
          swatches={FACE_SWATCHES}
        />
      </section>
    </div>
  );
}

function withRules<T extends { rules?: Rule[] }>(thing: T, rules: Rule[]): T {
  const next = { ...thing };
  if (rules.length > 0) next.rules = rules;
  else delete next.rules;
  return next;
}

/** A layout drawn from its own places, so the picture is never out of date. */
function LayoutThumb({ layout }: { layout: FaceLayout }) {
  const size = 44;
  const places = placeFace(layout, size, size, 0.42);
  return (
    <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
      <rect x={0.5} y={0.5} width={size - 1} height={size - 1} rx={6} fill="none" stroke="currentColor" strokeOpacity={0.35} />
      {LAYOUT_SLOTS[layout]
        .filter((slot) => slot !== "SLOT_BADGE")
        .map((slot) => {
          const r = places[slot];
          return (
            <rect
              key={slot}
              x={r.x}
              y={r.y}
              width={Math.max(1, r.w)}
              height={Math.max(1, r.h)}
              rx={1.5}
              fill="currentColor"
              fillOpacity={slot === "SLOT_VISUAL" || slot === "SLOT_VALUE" ? 0.28 : 0.6}
            />
          );
        })}
    </svg>
  );
}

function ElementCard({
  index,
  count,
  element,
  layout,
  open,
  onToggle,
  onChange,
  onMove,
  onRemove,
  onFree,
  variables,
}: {
  index: number;
  count: number;
  element: FaceElement;
  layout: FaceLayout;
  open: boolean;
  onToggle: () => void;
  onChange: (element: FaceElement) => void;
  onMove: (delta: -1 | 1) => void;
  onRemove: () => void;
  /** Take it out of its place, to be put anywhere: absent where there is no canvas. */
  onFree?: () => void;
  variables: readonly VariableChoice[];
}) {
  const kind = element.kind ?? "ELEMENT_KIND_TEXT";
  const slot = element.slot ?? "SLOT_TITLE";
  const set = (change: Partial<FaceElement>) => onChange(compactElement({ ...element, ...change }));
  const summary =
    kind === "ELEMENT_KIND_GRAPH" || kind === "ELEMENT_KIND_TIMER" ? element.variable : kind === "ELEMENT_KIND_CLOCK" ? (element.clock?.analog ? "with hands" : element.clock?.format || "HH:MM") : element.template;
  // Places this layout offers, plus wherever the element already is.
  const slots = Array.from(new Set<Slot>([...LAYOUT_SLOTS[layout], slot]));

  return (
    <div className={cn("rounded-xl border transition-colors", open ? "border-line-strong bg-plate-2" : "border-line bg-plate")}>
      <div className="flex items-center gap-2 px-2.5 py-2">
        <button type="button" onClick={onToggle} aria-expanded={open} className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left">
          <span className="lbl w-12 shrink-0">{kindName(kind)}</span>
          <span className="min-w-0 flex-1 truncate font-mono text-[12px] text-ink-2">{summary || "—"}</span>
          <span className="shrink-0 text-[11px] text-faint">{element.box ? "Anywhere" : SLOT_NAMES[slot]}</span>
        </button>
        <IconButton label="Earlier (drawn under)" disabled={index === 0} onClick={() => onMove(-1)}>↑</IconButton>
        <IconButton label="Later (drawn on top)" disabled={index === count - 1} onClick={() => onMove(1)}>↓</IconButton>
        <IconButton label="Remove" onClick={onRemove}>
          <Glyph name="close" size={12} />
        </IconButton>
      </div>
      {open ? (
        <div className="flex flex-col gap-3 border-t border-line px-2.5 pt-2.5 pb-3">
          <Row label="Place">
            <select
              aria-label="Place"
              value={element.box ? "ANYWHERE" : slot}
              onChange={(event) => (event.target.value === "ANYWHERE" ? onFree?.() : set({ slot: event.target.value as Slot, box: undefined }))}
              className={cn(control, "h-8")}
            >
              {slots.map((s) => (
                <option key={s} value={s}>
                  {SLOT_NAMES[s]}
                </option>
              ))}
              {onFree || element.box ? <option value="ANYWHERE">Anywhere — drag it on the key</option> : null}
            </select>
          </Row>

          {kind === "ELEMENT_KIND_TEXT" ? (
            <>
              <ValueInput label="Shows" value={element.template ?? ""} variables={variables} placeholder="CPU {{ system.cpu.usage | round }}%" onCommit={(template) => set({ template })} />
              <Hint>Text, a live value in double braces, or both. Filters: round, pct, currency, number, time, duration, upper, default, map.</Hint>
              <TextStyleRow element={element} onChange={set} />
            </>
          ) : null}

          {kind === "ELEMENT_KIND_ICON" ? (
            <>
              <IconPicker value={element.template} onPick={(icon) => set({ template: icon ?? "" })} />
              <ValueInput label="Icon" value={element.template ?? ""} variables={variables} kinds={["text"]} placeholder="or follow a value: {{media.current.art}}" onCommit={(template) => set({ template })} />
            </>
          ) : null}

          {kind === "ELEMENT_KIND_BAR" || kind === "ELEMENT_KIND_GAUGE" ? (
            <>
              <ValueInput label="Value" value={element.template ?? ""} variables={variables} kinds={["number"]} placeholder="{{system.cpu.usage}}" onCommit={(template) => set({ template })} />
              <RangeRow element={element} onChange={set} />
            </>
          ) : null}

          {kind === "ELEMENT_KIND_GRAPH" ? (
            <>
              <VariableInput label="Graphs" value={element.variable ?? ""} kinds={["number"]} variables={variables} onCommit={(variable) => set({ variable })} />
              <Row label="Drawn as">
                <Segmented
                  size="sm"
                  label="Graph style"
                  value={element.graph?.style ?? "GRAPH_STYLE_AREA"}
                  onChange={(style) => set({ graph: compactObject({ ...element.graph, style: style === "GRAPH_STYLE_AREA" ? undefined : style }) })}
                  options={[
                    { value: "GRAPH_STYLE_AREA", label: "Area" },
                    { value: "GRAPH_STYLE_LINE", label: "Line" },
                    { value: "GRAPH_STYLE_BARS", label: "Bars" },
                  ]}
                />
              </Row>
              <Row label="Last">
                <DraftInput
                  aria-label="Points shown"
                  value={String(element.graph?.points || 60)}
                  onCommit={(text) => {
                    const points = Math.max(2, Math.min(600, Math.round(Number(text) || 60)));
                    set({ graph: compactObject({ ...element.graph, points: points === 60 ? undefined : points }) });
                  }}
                  className={cn(control, "h-8 w-20 text-right font-mono")}
                />
                <span className="text-[12px] text-muted">seconds</span>
              </Row>
              <RangeRow element={element} onChange={set} automatic />
            </>
          ) : null}

          {kind === "ELEMENT_KIND_CLOCK" ? <ClockRows element={element} onChange={set} /> : null}

          {kind === "ELEMENT_KIND_TIMER" ? (
            <>
              <Row label="Timer">
                <DraftInput
                  aria-label="Which timer"
                  value={element.variable ?? ""}
                  placeholder="focus"
                  onCommit={(name) => set({ variable: name.replace(/[^A-Za-z0-9_-]/g, "") })}
                  className={cn(control, "h-8 flex-1 font-mono")}
                />
              </Row>
              <Hint>Start, pause and reset it with the Timers actions (under Does), naming the same timer. Its values read as timer.name.left, .running and .done.</Hint>
            </>
          ) : null}

          <ColorPicker
            label="Colour"
            swatches={kind === "ELEMENT_KIND_TEXT" || kind === "ELEMENT_KIND_ICON" || kind === "ELEMENT_KIND_CLOCK" || kind === "ELEMENT_KIND_TIMER" ? INK_SWATCHES : FACE_SWATCHES.concat(INK_SWATCHES)}
            value={element.color}
            onChange={(color) => set({ color })}
          />

          <div className="flex flex-col gap-1.5">
            <span className="text-[12px] text-muted">Rules</span>
            <RulesEditor rules={element.rules ?? []} onChange={(rules) => onChange(withRules(element, rules))} variables={variables} what="its colour" swatches={INK_SWATCHES.concat(FACE_SWATCHES)} />
          </div>
        </div>
      ) : null}
    </div>
  );
}

function compactElement(element: FaceElement): FaceElement {
  const next = compactObject(element);
  if (next.slot === "SLOT_TITLE") delete next.slot;
  if (next.kind === "ELEMENT_KIND_TEXT") delete next.kind;
  return next;
}

/** Drop what is unset, as the wire does. */
function compactObject<T extends object>(value: T): T {
  return Object.fromEntries(
    Object.entries(value).filter(([, field]) => field !== undefined && field !== "" && field !== false && field !== 0 && !(typeof field === "object" && field !== null && !Array.isArray(field) && Object.keys(field).length === 0)),
  ) as T;
}

function TextStyleRow({ element, onChange }: { element: FaceElement; onChange: (change: Partial<FaceElement>) => void }) {
  const text = element.text ?? {};
  const set = (change: Partial<NonNullable<FaceElement["text"]>>) => onChange({ text: compactObject({ ...text, ...change }) });
  return (
    <div className="flex flex-col gap-2">
      <div className="grid grid-cols-2 gap-2">
        <select aria-label="Size" value={text.size ?? "TEXT_SIZE_AUTO"} onChange={(event) => set({ size: event.target.value === "TEXT_SIZE_AUTO" ? undefined : (event.target.value as never) })} className={cn(control, "h-8")}>
          <option value="TEXT_SIZE_AUTO">Size: as placed</option>
          <option value="TEXT_SIZE_XS">Tiny</option>
          <option value="TEXT_SIZE_S">Small</option>
          <option value="TEXT_SIZE_M">Medium</option>
          <option value="TEXT_SIZE_L">Large</option>
          <option value="TEXT_SIZE_XL">Huge</option>
          <option value="TEXT_SIZE_FIT">As big as fits</option>
        </select>
        <select aria-label="Font" value={text.font ?? "FONT_SANS"} onChange={(event) => set({ font: event.target.value === "FONT_SANS" ? undefined : (event.target.value as never) })} className={cn(control, "h-8")}>
          <option value="FONT_SANS">Sans</option>
          <option value="FONT_MONO">Mono (steady digits)</option>
          <option value="FONT_SERIF">Serif</option>
        </select>
      </div>
      <Row label="Align">
        <Segmented
          size="sm"
          label="Alignment"
          value={text.align ?? "TEXT_ALIGN_AUTO"}
          onChange={(align) => set({ align: align === "TEXT_ALIGN_AUTO" ? undefined : align })}
          options={[
            { value: "TEXT_ALIGN_AUTO", label: "Auto" },
            { value: "TEXT_ALIGN_START", label: "Left" },
            { value: "TEXT_ALIGN_CENTER", label: "Centre" },
            { value: "TEXT_ALIGN_END", label: "Right" },
          ]}
        />
      </Row>
      <Row label="Style">
        <div className="flex flex-1 flex-wrap items-center gap-x-4 gap-y-2">
          <label className="flex items-center gap-1.5 text-[12px] text-muted">
            Bold <Switch size="sm" label="Bold" checked={text.bold ?? false} onChange={(bold) => set({ bold: bold || undefined })} />
          </label>
          <label className="flex items-center gap-1.5 text-[12px] text-muted" title="Too long for its place: one line, scrolling sideways">
            Scroll <Switch size="sm" label="Scroll when too long" checked={text.marquee ?? false} onChange={(marquee) => set({ marquee: marquee || undefined })} />
          </label>
          <label className="ml-auto flex items-center gap-1.5 text-[12px] text-muted" title="Most lines it wraps to (0: as placed)">
            Lines
            <DraftInput aria-label="Lines" value={String(text.maxLines ?? 0)} onCommit={(value) => set({ maxLines: Math.max(0, Math.min(4, Math.round(Number(value) || 0))) || undefined })} className={cn(control, "h-8 w-12 text-center font-mono")} />
          </label>
        </div>
      </Row>
    </div>
  );
}

function RangeRow({ element, onChange, automatic }: { element: FaceElement; onChange: (change: Partial<FaceElement>) => void; automatic?: boolean }) {
  return (
    <Row label="From">
      <DraftInput aria-label="Lowest" value={String(element.min ?? 0)} onCommit={(text) => onChange({ min: Number(text) || undefined })} className={cn(control, "h-8 w-20 text-right font-mono")} />
      <span className="text-[12px] text-muted">to</span>
      <DraftInput aria-label="Highest" value={String(element.max ?? 0)} onCommit={(text) => onChange({ max: Number(text) || undefined })} className={cn(control, "h-8 w-20 text-right font-mono")} />
      <span className="text-[11px] text-faint">{automatic ? "0 to 0: fits the data" : "0 to 0: 0–100"}</span>
    </Row>
  );
}

const ZONES = ["UTC", "Europe/London", "Europe/Berlin", "America/New_York", "America/Chicago", "America/Los_Angeles", "Asia/Kolkata", "Asia/Tokyo", "Asia/Singapore", "Australia/Sydney"];

function ClockRows({ element, onChange }: { element: FaceElement; onChange: (change: Partial<FaceElement>) => void }) {
  const clock = element.clock ?? {};
  const set = (change: Partial<NonNullable<FaceElement["clock"]>>) => onChange({ clock: compactObject({ ...clock, ...change }) });
  return (
    <>
      <Row label="Face">
        <Segmented size="sm" label="Clock face" value={clock.analog ? "analog" : "digital"} onChange={(face) => set({ analog: face === "analog" || undefined })} options={[{ value: "digital", label: "Digital" }, { value: "analog", label: "Hands" }]} />
        <label className="ml-auto flex items-center gap-1.5 text-[12px] text-muted">
          Seconds <Switch size="sm" label="Seconds" checked={clock.seconds ?? false} onChange={(seconds) => set({ seconds: seconds || undefined })} />
        </label>
      </Row>
      {!clock.analog ? (
        <Row label="Format">
          <DraftInput aria-label="Format" value={clock.format ?? ""} placeholder="%H:%M" onCommit={(format) => set({ format: format.trim() || undefined })} className={cn(control, "h-8 flex-1 font-mono")} />
        </Row>
      ) : null}
      <Row label="Zone">
        <DraftInput aria-label="Time zone" list="fd-zones" value={clock.zone ?? ""} placeholder="the tablet's own" onCommit={(zone) => set({ zone: zone.trim() || undefined })} className={cn(control, "h-8 flex-1")} />
        <datalist id="fd-zones">
          {ZONES.map((zone) => (
            <option key={zone} value={zone} />
          ))}
        </datalist>
      </Row>
      {!clock.analog ? <Hint>%H hour · %M minute · %S second · %-I 12-hour · %p AM/PM · %a day · %d date · %b month.</Hint> : null}
    </>
  );
}

const FACE_MODES = ["default", "solid", "gradient", "image", "transparent"] as const;

function BackgroundEditor({ background, onChange, variables, fill }: { background: Background; onChange: (background: Background) => void; variables: readonly VariableChoice[]; fill: boolean }) {
  const set = (change: Partial<Background>) => onChange({ ...background, ...change });
  return (
    <section className="flex flex-col gap-2.5">
      <Label>{fill ? "Background" : "Behind it"}</Label>
      {fill ? <FillPicker label="Fill" value={fillOf(background)} modes={FACE_MODES} variables={variables} onChange={(next) => onChange(toBackground(next, background) ?? {})} /> : null}
      {!background.transparent ? (
        <>
          <VariableInput label="Graph behind" value={background.graph ?? ""} kinds={["number"]} variables={variables} onCommit={(graph) => set({ graph })} />
          {background.graph ? <ColorPicker label="Graph" swatches={INK_SWATCHES.concat(FACE_SWATCHES)} value={background.graphColor} onChange={(graphColor) => set({ graphColor })} /> : null}
        </>
      ) : null}
    </section>
  );
}

/**
 * "While … use …": the first rule that holds wins. `when` is written as a
 * comparison over live values; the agent checks it on save.
 */
function RulesEditor({
  rules,
  onChange,
  variables,
  what,
  swatches,
}: {
  rules: Rule[];
  onChange: (rules: Rule[]) => void;
  variables: readonly VariableChoice[];
  what: string;
  swatches: readonly { color: string; name: string }[];
}) {
  const update = (index: number, change: Partial<Rule>) =>
    onChange(rules.map((rule, at) => (at === index ? compactObject({ ...rule, ...change }) : rule)));
  return (
    <div className="flex flex-col gap-2">
      {rules.map((rule, index) => (
        <div key={index} className="flex flex-col gap-2 rounded-lg border border-line bg-well/40 p-2">
          <div className="flex items-center gap-2">
            <span className="text-[12px] text-muted">While</span>
            <ExpressionInput label="While" value={rule.when ?? ""} variables={variables} placeholder="system.cpu.usage > 80" onCommit={(when) => update(index, { when: when.replace(/^\{\{\s*|\s*\}\}$/g, "") })} className="flex-1 font-mono" />
            <IconButton label="Remove rule" onClick={() => onChange(rules.filter((_, at) => at !== index))}>
              <Glyph name="close" size={12} />
            </IconButton>
          </div>
          <ColorPicker label="Use" swatches={swatches} value={rule.color} onChange={(color) => update(index, { color })} />
          <label className="flex items-center gap-1.5 self-end text-[12px] text-muted">
            Pulse <Switch size="sm" label="Pulse" checked={rule.pulse ?? false} onChange={(pulse) => update(index, { pulse })} />
          </label>
        </div>
      ))}
      <button
        type="button"
        onClick={() => onChange([...rules, { when: "", color: "#7f1d1d" }])}
        className="flex cursor-pointer items-center justify-center gap-1 rounded-lg border border-dashed border-line-strong px-2 py-1.5 text-[12px] text-muted hover:border-signal hover:text-signal"
      >
        <Glyph name="plus" size={11} /> Change {what} while…
      </button>
    </div>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center gap-2">
      <span className="w-14 shrink-0 text-[12px] text-muted">{label}</span>
      {children}
    </div>
  );
}

function Hint({ children }: { children: ReactNode }) {
  return <p className="m-0 text-[11px] leading-snug text-faint">{children}</p>;
}

function IconButton({ label, onClick, disabled, children }: { label: string; onClick: () => void; disabled?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
      className="flex size-6 shrink-0 cursor-pointer items-center justify-center rounded-md text-[12px] text-muted hover:bg-plate-2 hover:text-ink disabled:cursor-default disabled:opacity-30"
    >
      {children}
    </button>
  );
}
