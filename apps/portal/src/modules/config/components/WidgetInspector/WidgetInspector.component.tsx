import type { Orientation, PlacedSpan } from "@flexdeck/grid";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useState, type CSSProperties, type ReactNode } from "react";

import { Glyph, HoldToConfirm, Key, Label, Notice, Segmented, Switch, cn, screen, spring } from "@flexdeck/ui";

import { Icon } from "../../../../common/components/Icon/Icon.component.tsx";
import type { ActionDefinition, ArgumentChoices, SliderOrientation, VariableChoice, WidgetPreview, WireConfig, WireWidget } from "../../../../services/api/api.ts";
import { editFace, hasFace, setCustomFace, setKeyFill } from "../../lib/face.edit.lib.ts";
import { fillOf } from "../../lib/fill.lib.ts";
import { FaceEditor } from "../FaceEditor/FaceEditor.component.tsx";
import { FaceView } from "../FaceView/FaceView.component.tsx";
import { IconPicker } from "../IconPicker/IconPicker.component.tsx";
import { KeyGallery } from "../KeyGallery/KeyGallery.component.tsx";
import { suggestedActions } from "../../lib/presets.lib.ts";
import { LooksEditor } from "../LooksEditor/LooksEditor.component.tsx";
import { ZonesEditor } from "../ZonesEditor/ZonesEditor.component.tsx";
import type { LoadChoices } from "../../hooks/useArgumentChoices/useArgumentChoices.hook.ts";
import { relightActive, setHideDot, setWidgetColors, stateVariableOf } from "../../lib/editing.lib.ts";
import { INK_SWATCHES, faceColors } from "../../lib/face.lib.ts";
import { ColorPicker } from "../FillPicker/ColorPicker.component.tsx";
import { FillPicker } from "../FillPicker/FillPicker.component.tsx";
import { HOLD_DOWN, HOLD_UP, SLOT_GESTURES, gestureBlocks, holdsOf, setGestureBlocks, slotCounts, type Slot } from "../../lib/flow.lib.ts";
import {
  channelKeyOf,
  isSlider,
  setIconTemplate,
  setSliderChannel,
  setSliderLayout,
  setSliderTarget,
  setWidgetKind,
  sliderChannels,
  sliderTargetOf,
  sliderTargets,
} from "../../lib/slider.lib.ts";
import { WIDGET_STATES, type WidgetState } from "../../lib/state.lib.ts";
import { control, TemplatePreview, ValueInput, VariableInput } from "../Flow/fields.tsx";
import { FlowLane } from "../Flow/FlowLane.component.tsx";
import { FlowProvider } from "../Flow/FlowProvider.component.tsx";
import { summarize, useFlow } from "../Flow/flow.context.ts";
import { StepPicker } from "../StepPicker/StepPicker.component.tsx";

type WidgetEdit = (transform: (config: WireConfig, widgetId: string) => WireConfig) => void;

export interface WidgetInspectorProps {
  widget: WireWidget;
  catalogue: ActionDefinition[];
  variables: VariableChoice[];
  loadChoices?: LoadChoices;
  labelDraft: string;
  onLabelChange: (value: string) => void;
  onCommitLabel: () => void;
  onIcon: (icon: string | null) => void;
  onBinding: (binding: { stateVariable: string; labelTemplate: string }) => void;
  onEdit: WidgetEdit;
  previewState: WidgetState;
  onPreviewState: (state: WidgetState) => void;
  /** What the agent says this key shows now, or why it cannot. */
  preview?: WidgetPreview;
  /** The state the editor draws the key in; `""` for whichever applies. */
  previewLook?: string;
  onPreviewLook?: (lookId: string) => void;
  /** Which layout is being edited, and where this key sits in it. */
  orientation: Orientation;
  placement: PlacedSpan | null;
  /** Let portrait place this key for you again. */
  onUnpin: () => void;
  onDuplicate: () => void;
  onRemove: () => void;
  onClose: () => void;
}

type Tab = "does" | "looks" | "gallery" | "slider";

const SLOTS: readonly { slot: Slot; label: string; hint: string }[] = [
  { slot: "tap", label: "Tap", hint: "A quick press" },
  { slot: "double", label: "Double tap", hint: "Two quick presses" },
  { slot: "long", label: "Long press", hint: "Press and keep it down" },
  { slot: "hold", label: "Hold ↓↑", hint: "One thing down, another up" },
];

/**
 * Everything about one key, as a device would put it: the key itself up top,
 * what it DOES (a flow per gesture), and how it LOOKS (states, live label,
 * icon). A slider swaps "does" for what it controls.
 */
export function WidgetInspector(p: WidgetInspectorProps) {
  const slider = isSlider(p.widget);
  const first = (): Tab => (slider ? "slider" : "does");
  const [tab, setTab] = useState<Tab>(first);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => setTab(first()), [p.widget.id, slider]);

  return (
    <FlowProvider catalogue={p.catalogue} variables={p.variables} loadChoices={p.loadChoices}>
      <div className="flex min-h-0 flex-1 flex-col">
        <Header {...p} slider={slider} />
        {/* What the key does sits on the left — picking it by hand or from the
            gallery; how it looks is its own pill on the right. */}
        <div className="flex items-center justify-between gap-2 px-5 pb-3">
          <Segmented
            label="Behaviour"
            value={tab}
            onChange={setTab}
            options={
              slider
                ? [{ value: "slider", label: "Slider" }]
                : [
                    { value: "does", label: "Does" },
                    { value: "gallery", label: "Gallery" },
                  ]
            }
          />
          <Segmented label="Appearance" value={tab} onChange={setTab} options={[{ value: "looks", label: "Looks" }]} />
        </div>
        {p.preview?.state?.unavailable ? (
          <div className="px-5 pb-3">
            <Notice tone="warn">{`Disabled: ${p.preview.state.unavailable}. Pick another action under Does, or update the plugin.`}</Notice>
          </div>
        ) : null}
        <div className="min-h-0 flex-1 overflow-y-auto px-5 pb-5">
          <AnimatePresence mode="wait" initial={false}>
            <motion.div key={tab} variants={screen} initial="initial" animate="animate" exit="exit">
              {tab === "does" ? <Does widget={p.widget} onEdit={p.onEdit} /> : null}
              {tab === "looks" ? <Looks {...p} /> : null}
              {tab === "gallery" ? <KeyGallery widget={p.widget} onEdit={p.onEdit} /> : null}
              {tab === "slider" ? <SliderTab widget={p.widget} catalogue={p.catalogue} variables={p.variables} loadChoices={p.loadChoices} onEdit={p.onEdit} /> : null}
            </motion.div>
          </AnimatePresence>
        </div>
        <footer className="flex flex-wrap items-center gap-2 border-t border-line px-5 py-3">
          <Key size="sm" onClick={p.onDuplicate}>
            <Glyph name="copy" size={13} /> Duplicate
          </Key>
          <HoldToConfirm size="sm" tone="danger" className="ml-auto" onConfirm={p.onRemove}>
            Hold to delete
          </HoldToConfirm>
        </footer>
      </div>
    </FlowProvider>
  );
}

function Header({
  widget,
  preview,
  previewLook,
  labelDraft,
  onLabelChange,
  onCommitLabel,
  onEdit,
  orientation,
  placement,
  onUnpin,
  onClose,
  slider,
}: WidgetInspectorProps & { slider: boolean }) {
  const counts = slotCounts(widget);
  const lit = Object.values(counts).some((count) => count > 0) || stateVariableOf(widget) !== "";
  return (
    <div className="flex flex-col gap-4 p-5">
      <div className="flex items-start justify-between">
        <Label>
          Key · <span className="text-faint">{widget.id}</span>
        </Label>
        <button type="button" aria-label="Close" onClick={onClose} className="-mt-1 -mr-1 cursor-pointer rounded-lg p-1.5 text-muted hover:bg-plate-2 hover:text-ink">
          <Glyph name="close" size={16} />
        </button>
      </div>
      <div className="flex items-center gap-4">
        <motion.div
          layout
          className="relative flex size-[92px] shrink-0 flex-col items-center justify-center gap-1.5 rounded-[18px] border border-[var(--fd-key-line)] bg-linear-to-b from-[var(--fd-key-top)] to-[var(--fd-key-bottom)] px-1.5 text-center shadow-key"
          style={keyStyle(widget, false)}
        >
          {slider ? (
            <>
              {widget.props?.icon ? <Icon name={widget.props.icon} size={26} decorative /> : null}
              <span className="line-clamp-2 text-[11px] leading-tight">{labelDraft || "Untitled"}</span>
            </>
          ) : (
            <FaceView widget={widget} lookId={previewLook || undefined} w={92} h={92} unit={0.62} radius={18} preview={preview} />
          )}
          {lit ? <span className="absolute top-2 right-2 size-1.5 rounded-full bg-signal text-signal led-glow" /> : null}
        </motion.div>
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <input
            aria-label="Name"
            value={labelDraft}
            placeholder="Name this key"
            onChange={(event) => onLabelChange(event.target.value)}
            onBlur={onCommitLabel}
            onKeyDown={(event) => event.key === "Enter" && onCommitLabel()}
            className="h-11 w-full rounded-xl border border-line bg-well px-3 text-lg font-semibold shadow-well outline-none focus:outline-2 focus:outline-signal"
          />
          <div className="flex items-center gap-2">
            <Segmented
              size="sm"
              label="Kind"
              value={slider ? "slider" : "button"}
              onChange={(kind) => onEdit((config, id) => setWidgetKind(config, id, kind))}
              options={[
                { value: "button", label: "Key" },
                { value: "slider", label: "Slider" },
              ]}
            />
          </div>
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 font-mono text-[10px] tracking-[0.12em] text-faint">
        <span className="text-muted">{orientation === "portrait" ? "PORTRAIT" : "LANDSCAPE"}</span>
        {placement ? (
          <span>
            C{placement.col + 1} · R{placement.row + 1} · {placement.w}×{placement.h}
          </span>
        ) : (
          <span className="text-warn">NO ROOM IN THIS GRID</span>
        )}
        {orientation === "portrait" && placement ? (
          placement.assumed ? (
            <span title="Placed for you from landscape until you drag it somewhere">· AUTO-PLACED</span>
          ) : (
            <button type="button" onClick={onUnpin} className="cursor-pointer text-signal hover:underline" title="Let portrait place it for you again">
              · PINNED — UNPIN
            </button>
          )
        ) : null}
      </div>
    </div>
  );
}

/* ---- DOES ---------------------------------------------------------------- */

function Does({ widget, onEdit }: { widget: WireWidget; onEdit: WidgetEdit }) {
  const [slot, setSlot] = useState<Slot>("tap");
  useEffect(() => setSlot("tap"), [widget.id]);
  const counts = slotCounts(widget);
  const hold = holdsOf(widget);
  const { catalogue } = useFlow();
  const describeBlock = (block: Parameters<typeof summarize>[0]) => summarize(block, catalogue);
  const suggestions = suggestedActions(widget, catalogue);

  const summary = (target: Slot): string => {
    if (target === "hold") {
      const down = gestureBlocks(widget, HOLD_DOWN);
      return down[0] ? describeBlock(down[0]) : counts.hold > 0 ? "On release only" : "+ Assign";
    }
    const blocks = gestureBlocks(widget, SLOT_GESTURES[target]);
    return blocks[0] ? describeBlock(blocks[0]) + (blocks.length > 1 ? `  +${blocks.length - 1}` : "") : "+ Assign";
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="grid grid-cols-2 gap-2">
        {SLOTS.map((entry) => {
          const on = entry.slot === slot;
          const filled = counts[entry.slot] > 0;
          return (
            <button
              key={entry.slot}
              type="button"
              aria-pressed={on}
              onClick={() => setSlot(entry.slot)}
              className={cn(
                "relative flex min-h-[78px] cursor-pointer flex-col gap-1.5 rounded-[14px] border p-3 text-left transition-colors",
                on ? "border-signal bg-signal-soft/50" : "border-line bg-plate hover:border-line-strong",
              )}
            >
              {on ? <motion.span layoutId="slot-ring" transition={spring.panel} className="absolute inset-0 rounded-[14px] ring-2 ring-signal/25" /> : null}
              <span className="flex items-center justify-between">
                <span className={cn("lbl", on && "text-signal")}>{entry.label}</span>
                {filled ? <span className="size-1.5 rounded-full bg-signal text-signal led-glow" /> : null}
              </span>
              <span className={cn("line-clamp-2 text-[12px] leading-snug", filled ? "text-ink-2" : "text-faint")}>{summary(entry.slot)}</span>
            </button>
          );
        })}
      </div>

      {slot === "tap" && counts.tap === 0 && suggestions.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="text-[11px] text-faint">Suggested:</span>
          {suggestions.map((suggestion) => (
            <button
              key={suggestion.label}
              type="button"
              onClick={() => onEdit((config, id) => relightActive(setGestureBlocks(config, id, SLOT_GESTURES.tap, [suggestion.block]), id, catalogue))}
              className="cursor-pointer rounded-full border border-line bg-plate px-2.5 py-1 text-[11px] text-muted hover:border-signal hover:text-ink"
            >
              {suggestion.label}
            </button>
          ))}
        </div>
      ) : null}

      {hold && slot !== "hold" ? (
        <Notice tone="warn">
          This key is a hold, so the tablet sends “down” and “up” instead of taps. Giving the tap something to do turns the hold off.
        </Notice>
      ) : null}

      <AnimatePresence mode="wait" initial={false}>
        <motion.div key={slot} initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -6 }} transition={{ duration: 0.18 }} className="flex flex-col gap-4">
          {slot === "hold" ? (
            <>
              <LaneSection title="As it goes down" hint="Push-to-talk: open the mic here.">
                <FlowLane single blocks={gestureBlocks(widget, HOLD_DOWN)} onChange={(blocks) => onEdit((config, id) => setGestureBlocks(config, id, HOLD_DOWN, blocks))} empty="Add what happens on press" />
              </LaneSection>
              <LaneSection title="As it comes up" hint="Also runs if the tablet disconnects mid-hold, so a mic is never left open.">
                <FlowLane single blocks={gestureBlocks(widget, HOLD_UP)} onChange={(blocks) => onEdit((config, id) => setGestureBlocks(config, id, HOLD_UP, blocks))} empty="Add what happens on release" />
              </LaneSection>
            </>
          ) : (
            <LaneSection title={`${SLOTS.find((entry) => entry.slot === slot)?.label} runs`} hint={SLOTS.find((entry) => entry.slot === slot)?.hint}>
              <FlowLane
                single
                blocks={gestureBlocks(widget, SLOT_GESTURES[slot])}
                onChange={(blocks) => onEdit((config, id) => relightActive(setGestureBlocks(config, id, SLOT_GESTURES[slot], blocks), id, catalogue))}
                empty="Assign an action"
              />
            </LaneSection>
          )}
        </motion.div>
      </AnimatePresence>

      {(widget.face?.zones ?? []).map((zone) => (
        <LaneSection key={zone.name} title={`Its “${zone.name}” button runs`} hint="A tap on that part of the bottom strip — never the key's own.">
          <FlowLane
            single
            blocks={gestureBlocks(widget, SLOT_GESTURES.tap, zone.name)}
            onChange={(blocks) => onEdit((config, id) => setGestureBlocks(config, id, SLOT_GESTURES.tap, blocks, zone.name))}
            empty="Assign an action"
          />
        </LaneSection>
      ))}
    </div>
  );
}

function LaneSection({ title, hint, children }: { title: string; hint?: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2.5">
      <div className="flex items-baseline justify-between gap-3">
        <Label className="text-ink">{title}</Label>
        {hint ? <span className="truncate text-[11px] text-faint">{hint}</span> : null}
      </div>
      {children}
    </section>
  );
}

/* ---- LOOKS --------------------------------------------------------------- */

function Looks(p: WidgetInspectorProps) {
  const { widget, variables, onEdit, preview } = p;
  const custom = hasFace(widget);
  return (
    <div className="flex flex-col gap-5">
      <Segmented
        size="sm"
        label="Face"
        value={custom ? "custom" : "simple"}
        onChange={(mode) => onEdit((config, id) => setCustomFace(config, id, mode === "custom"))}
        options={[
          { value: "simple", label: "Simple" },
          { value: "custom", label: "Custom face" },
        ]}
      />
      <section className="flex flex-col gap-2.5">
        <Label>Background</Label>
        <FillPicker
          label="Fill"
          value={widget.face ? fillOf(widget.face.background) : fillOf(widget.props?.bgColor ? { color: widget.props.bgColor } : undefined)}
          modes={KEY_FILL_MODES}
          variables={variables}
          onChange={(fill) => onEdit((config, id) => setKeyFill(config, id, fill))}
          hint={custom ? undefined : "A fade, an image or none gives the key a face of its own."}
        />
      </section>
      {custom && widget.face ? (
        <>
          <FaceEditor
            fill={false}
            face={widget.face}
            onChange={(change) => onEdit((config, id) => editFace(config, id, "", change))}
            variables={variables}
            error={preview?.error}
            canvas={{ widget, preview }}
          />
          <ZonesEditor widget={widget} onEdit={onEdit} />
        </>
      ) : (
        <>
          {preview?.error ? <Notice tone="warn">{preview.error}</Notice> : null}
          <SimpleLooks {...p} />
        </>
      )}
      <LooksEditor widget={widget} onEdit={onEdit} variables={variables} previewLook={p.previewLook ?? ""} onPreviewLook={p.onPreviewLook ?? (() => {})} />
      <section className="flex flex-col gap-2.5">
        <Label>Preview as</Label>
        <Segmented size="sm" label="Preview state" value={p.previewState} onChange={p.onPreviewState} options={WIDGET_STATES.map((state) => ({ value: state, label: state }))} />
        <p className="m-0 text-[11px] text-faint">Only the editor shows this; press the key on the tablet to see the real one.</p>
      </section>
    </div>
  );
}

const KEY_FILL_MODES = ["default", "solid", "gradient", "image", "transparent"] as const;

/** One label, one icon, two colours: the key as most people want it. */
function SimpleLooks({ widget, variables, onBinding, onEdit, onIcon, previewState }: WidgetInspectorProps) {
  const bound = stateVariableOf(widget);
  const lights = (widget.states?.looks ?? []).some((look) => look.id === "on");
  const template = widget.props?.labelTemplate ?? "";
  const iconTemplate = widget.props?.iconTemplate ?? "";

  return (
    <div className="flex flex-col gap-5">
      <section className="flex flex-col gap-2.5">
        <div className="flex items-center justify-between gap-3">
          <Label>Lights up while</Label>
          {lights ? (
            <span className="flex items-center gap-2 text-[11px] text-faint">
              Corner dot
              <Switch size="sm" label="Corner dot" checked={!widget.props?.hideDot} onChange={(show) => onEdit((config, id) => setHideDot(config, id, !show))} />
            </span>
          ) : null}
        </div>
        <div className="grid grid-cols-2 gap-2">
          <LookCard title="Off" active={!bound || previewState !== "on"} label={widget.props?.label} icon={widget.props?.icon} style={keyStyle(widget, false)} />
          <LookCard title="On" lit dot={!widget.props?.hideDot} active={Boolean(bound) && previewState === "on"} label={widget.props?.label} icon={widget.props?.icon} style={keyStyle(widget, true)} />
        </div>
        <VariableInput
          label="Light up while"
          value={bound}
          kinds={["bool"]}
          variables={variables}
          onCommit={(stateVariable) => onBinding({ stateVariable, labelTemplate: template })}
        />
        <p className="m-0 text-[11px] text-faint">
          {bound
            ? `Lit while this is true: ${variables.find((variable) => variable.name === bound)?.description || bound}.`
            : "Pick a true/false value, e.g. obs.recording. A tap that toggles something fills this in itself."}
        </p>
      </section>

      <section className="flex flex-col gap-2.5">
        <Label>Label shows</Label>
        <ValueInput
          label="Label shows"
          value={template}
          variables={variables}
          placeholder="REC {{obs.record_time}}"
          onCommit={(labelTemplate) => onBinding({ stateVariable: bound, labelTemplate })}
        />
        {template ? <TemplatePreview text={template} /> : <p className="m-0 text-[11px] text-faint">Put a value in double braces and the tablet keeps it current.</p>}
      </section>

      <section className="flex flex-col gap-2.5">
        <Label>Icon</Label>
        <IconPicker value={widget.props?.icon} onPick={onIcon} />
        <ValueInput
          label="Icon follows"
          value={iconTemplate}
          kinds={["text"]}
          variables={variables}
          placeholder="or follow a value: {{media.current.art}}"
          onCommit={(next) => onEdit((config, id) => setIconTemplate(config, id, next))}
        />
      </section>

      <section className="flex flex-col gap-2.5">
        <Label>Text</Label>
        <ColorPicker
          label="Colour"
          swatches={INK_SWATCHES}
          value={widget.props?.fgColor}
          onChange={(fgColor) => onEdit((config, id) => setWidgetColors(config, id, { fgColor }))}
        />
        <p className="m-0 text-[11px] text-faint">The face shows while the key is not lit; a lit key keeps the deck's glow.</p>
      </section>

    </div>
  );
}

function LookCard({ title, lit, dot = true, active, label, icon, style }: { title: string; lit?: boolean; dot?: boolean; active: boolean; label?: string; icon?: string; style?: CSSProperties }) {
  return (
    <div className={cn("flex flex-col gap-2 rounded-[14px] border p-2.5 transition-colors", active ? (lit ? "border-signal-line bg-signal-soft/50" : "border-line-strong bg-plate-2") : "border-line bg-plate opacity-70")}>
      <span className={cn("lbl", lit && "text-signal")}>{title}</span>
      <div
        className={cn(
          "relative flex h-16 flex-col items-center justify-center gap-1 rounded-xl border text-[11px]",
          lit ? "border-signal-line bg-signal-soft text-signal-hi" : "border-[var(--fd-key-line)] bg-linear-to-b from-[var(--fd-key-top)] to-[var(--fd-key-bottom)]",
        )}
        style={style}
      >
        {lit && dot ? <span className="absolute top-1.5 right-1.5 size-1.5 rounded-full bg-signal text-signal led-glow" /> : null}
        {icon ? <Icon name={icon} size={16} decorative /> : null}
        <span className="max-w-full truncate px-1">{label || "Key"}</span>
      </div>
    </div>
  );
}

/** The inline style that paints a key preview the way the tablet will. */
function keyStyle(widget: WireWidget, lit: boolean): CSSProperties {
  const { background, ink } = faceColors(widget, lit);
  return {
    ...(background ? { background } : {}),
    ...(ink ? { color: ink } : {}),
  };
}

/* ---- SLIDER -------------------------------------------------------------- */

const ORIENTATIONS: { value: SliderOrientation; label: string }[] = [
  { value: "SLIDER_ORIENTATION_AUTO", label: "Auto" },
  { value: "SLIDER_ORIENTATION_VERTICAL", label: "Up" },
  { value: "SLIDER_ORIENTATION_HORIZONTAL", label: "Across" },
];

/**
 * What a slider controls and how it moves. The channel list is everything the
 * desktop's plugins can put on a fader, by name; picking one wires the value,
 * the range, the value the thumb follows, and a tap on its name to mute.
 */
function SliderTab({
  widget,
  catalogue,
  variables,
  loadChoices,
  onEdit,
}: {
  widget: WireWidget;
  catalogue: ActionDefinition[];
  variables: VariableChoice[];
  loadChoices?: LoadChoices;
  onEdit: WidgetEdit;
}) {
  const targets = useMemo(() => sliderTargets(catalogue), [catalogue]);
  const target = sliderTargetOf(widget);
  const chosen = target ? targets.find((action) => action.plugin === target.plugin && action.action === target.action) : undefined;
  const [round, setRound] = useState(0);
  const [answers, setAnswers] = useState<{ choices: Record<string, ArgumentChoices>; loading: boolean }>({ choices: {}, loading: false });
  const keys = targets.map((action) => `${action.plugin}:${action.action}`).join("|");
  useEffect(() => {
    if (!loadChoices || targets.length === 0) return;
    let current = true;
    setAnswers((before) => ({ ...before, loading: true }));
    Promise.all(
      targets.map((action) =>
        loadChoices(action.plugin, action.action, {})
          .catch((error: unknown): ArgumentChoices => ({ arguments: {}, error: String(error) }))
          .then((answer) => [`${action.plugin}:${action.action}`, answer] as const),
      ),
    ).then((list) => current && setAnswers({ choices: Object.fromEntries(list), loading: false }));
    return () => {
      current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [keys, loadChoices, round]);

  const channels = useMemo(() => sliderChannels(targets, answers.choices), [targets, answers.choices]);
  const current = channelKeyOf(widget);
  const picked = channels.find((channel) => channel.key === current) ?? null;
  const [byHand, setByHand] = useState(false);
  useEffect(() => setByHand(false), [widget.id]);
  const custom = byHand || (target !== null && picked === null && !answers.loading);
  const [filter, setFilter] = useState("");
  const shown = channels.filter((channel) => `${channel.group} ${channel.label}`.toLowerCase().includes(filter.trim().toLowerCase()));
  const orientation = widget.slider?.orientation ?? "SLIDER_ORIENTATION_AUTO";
  const followed = widget.slider?.variable ?? "";

  return (
    <div className="flex flex-col gap-5">
      <section className="flex flex-col gap-2.5">
        <div className="flex items-center justify-between">
          <Label>Controls</Label>
          <button type="button" onClick={() => setRound((value) => value + 1)} className="lbl flex cursor-pointer items-center gap-1 hover:text-ink" title="A program appears once it plays sound">
            <Glyph name="refresh" size={11} /> Look again
          </button>
        </div>
        <input aria-label="Find a channel" placeholder="Find a channel: master, OBS, Spotify…" value={filter} onChange={(event) => setFilter(event.target.value)} className={control} />
        <div role="listbox" aria-label="Channels" className="flex max-h-60 flex-col gap-1 overflow-y-auto rounded-xl border border-line bg-well p-1.5 shadow-well">
          {answers.loading && channels.length === 0 ? <p className="m-0 p-3 text-[12px] text-muted">Finding channels…</p> : null}
          {!answers.loading && targets.length === 0 ? <p className="m-0 p-3 text-[12px] text-muted">No plugin on this desktop offers anything a slider can drive yet.</p> : null}
          {shown.map((channel) => {
            const on = channel.key === current && !custom;
            return (
              <button
                key={channel.key}
                type="button"
                role="option"
                aria-selected={on}
                onClick={() => {
                  setByHand(false);
                  onEdit((config, id) => setSliderChannel(config, id, channel, catalogue, picked));
                }}
                className={cn("flex cursor-pointer items-center gap-3 rounded-lg px-2.5 py-2 text-left text-[13px] transition-colors", on ? "bg-signal-soft text-signal-hi" : "hover:bg-plate")}
              >
                <span className="flex h-5 w-1.5 shrink-0 flex-col justify-end overflow-hidden rounded-full bg-line">
                  <span className={cn("w-full rounded-full", on ? "h-3/4 bg-signal" : "h-1/2 bg-faint")} />
                </span>
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="truncate">{channel.label}</span>
                  <span className={cn("truncate text-[11px]", on ? "text-signal-hi/70" : "text-muted")}>{channel.group}</span>
                </span>
              </button>
            );
          })}
          <button
            type="button"
            onClick={() => setByHand(true)}
            className={cn("cursor-pointer rounded-lg px-2.5 py-2 text-left text-[13px] text-muted hover:bg-plate", custom && "bg-plate text-ink")}
          >
            Set up by hand…
          </button>
        </div>
        {custom ? (
          <StepPicker
            label="Drag to set"
            actions={targets}
            value={target}
            hidden={chosen?.slider?.argument}
            loadChoices={loadChoices}
            onChange={(step) => onEdit((config, id) => setSliderTarget(config, id, step, catalogue))}
          />
        ) : null}
        <p className="m-0 text-[11px] text-faint">
          Runs {widget.slider?.min ?? 0}–{widget.slider?.max ?? 100}.{chosen?.slider?.mute ? " Tap its name on the tablet to mute." : ""}
        </p>
      </section>

      <section className="flex flex-col gap-2.5">
        <Label>Direction</Label>
        <Segmented
          size="sm"
          label="Slider direction"
          value={orientation}
          onChange={(value) => onEdit((config, id) => setSliderLayout(config, id, { orientation: value }))}
          options={ORIENTATIONS}
        />
      </section>

      <section className="flex flex-col gap-2.5">
        <Label>Thumb follows</Label>
        <VariableInput
          label="Thumb follows"
          value={followed}
          kinds={["number"]}
          variables={variables}
          onCommit={(variable) => onEdit((config, id) => setSliderLayout(config, id, { variable }))}
        />
        <p className="m-0 text-[11px] text-faint">So the thumb moves when the value changes elsewhere. Filled in for you when the channel reports one.</p>
      </section>
    </div>
  );
}
