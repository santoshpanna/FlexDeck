import type { ImageFit } from "@flexdeck/face";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { AngleDial, ColorArea, HueStrip, Popover, Segmented, cn } from "@flexdeck/ui";

import { usePictureUrl } from "../../../../hooks/usePictureUrl/usePictureUrl.hook.ts";
import type { VariableChoice } from "../../../../services/api/api.ts";
import { FACE_SWATCHES } from "../../lib/face.lib.ts";
import {
  FILL_MODE_LABELS,
  GRADIENT_PRESETS,
  fillCss,
  fillLabel,
  hexToHsv,
  hsvToHex,
  parseHex,
  switchMode,
  type Fill,
  type FillMode,
  type Hsv,
} from "../../lib/fill.lib.ts";
import { ValueInput } from "../Flow/fields.tsx";
import { PicturePicker } from "../PicturePicker/PicturePicker.component.tsx";

type Swatches = readonly { color: string; name: string }[];

export interface FillPickerProps {
  /** The row's name: "Background", "Text". */
  label: string;
  value: Fill;
  /** The modes this place can take, in the order the tabs show them. */
  modes: readonly FillMode[];
  /** Called when a change is meant to be kept: a click, a typed colour, the end of a drag. */
  onChange: (fill: Fill) => void;
  swatches?: Swatches;
  /** Offered when a picture follows a live value. Without them there is no such field. */
  variables?: readonly VariableChoice[];
  /** A line under the panel, for what this place does with the fill. */
  hint?: ReactNode;
}

/**
 * One row that says what fills something — a colour, a fade, a picture — and
 * opens a panel to change it. Drags show live in the panel and are kept when
 * they end, so one drag is one undo.
 */
export function FillPicker({ label, value, modes, onChange, swatches = FACE_SWATCHES, variables, hint }: FillPickerProps) {
  const anchor = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  // What the panel shows: the value, or a drag not yet kept.
  const [draft, setDraft] = useState<Fill>(value);
  const dragging = useRef(false);
  const valueKey = JSON.stringify(value);
  useEffect(() => {
    if (!dragging.current) setDraft(value);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [valueKey]);

  const change = (next: Fill, commit = true) => {
    setDraft(next);
    // A picture tab with no picture yet is a choice still being made: there
    // is nothing to keep, and keeping it would read back as "default".
    const keepable = !(next.kind === "image" && !next.image.trim());
    dragging.current = !commit || !keepable;
    if (commit && keepable && JSON.stringify(next) !== valueKey) {
      onChange(next);
      if (next.kind === "solid") rememberColor(next.color);
      if (next.kind === "gradient") {
        rememberColor(next.from);
        rememberColor(next.to);
      }
    }
  };

  const shown = open ? draft : value;
  return (
    <div className="flex items-center gap-2">
      <span className="w-14 shrink-0 text-[12px] text-muted">{label}</span>
      <button
        ref={anchor}
        type="button"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={`${label}: ${fillLabel(shown)}`}
        onClick={() => setOpen((was) => !was)}
        className={cn(
          "flex h-9 min-w-0 flex-1 cursor-pointer items-center gap-2 rounded-[9px] border bg-well px-1.5 text-left shadow-well transition-colors",
          open ? "border-signal" : "border-line hover:border-line-strong",
        )}
      >
        <FillChip fill={shown} />
        <span className="min-w-0 flex-1 truncate text-[12px]">{fillLabel(shown)}</span>
        <span aria-hidden="true" className={cn("pr-1 text-[10px] text-faint transition-transform", open && "rotate-180")}>
          ▾
        </span>
      </button>
      <Popover
        open={open}
        onClose={() => {
          setOpen(false);
          dragging.current = false;
          setDraft(value);
        }} anchor={anchor}
        label={`${label} fill`}
        width={340}
      >
        <FillPanel fill={draft} modes={modes} onChange={change} swatches={swatches} variables={variables} />
        {hint ? <p className="m-0 mt-3 text-[11px] leading-snug text-faint">{hint}</p> : null}
      </Popover>
    </div>
  );
}

/** A small square that shows a fill: its colour, fade or picture; "A" for the deck's own. */
export function FillChip({ fill, size = 24 }: { fill: Fill; size?: number }) {
  const picture = usePictureUrl(fill.kind === "image" && fill.image && !fill.image.includes("{{") ? fill.image : null);
  const css = fillCss(fill);
  return (
    <span
      aria-hidden="true"
      className={cn(
        "relative flex shrink-0 items-center justify-center overflow-hidden rounded-md border border-line font-mono text-[10px] text-faint",
        fill.kind === "default" && "bg-linear-to-b from-[var(--fd-key-top)] to-[var(--fd-key-bottom)]",
        fill.kind === "transparent" && "bg-[repeating-conic-gradient(var(--color-line)_0_25%,transparent_0_50%)] bg-[length:8px_8px]",
      )}
      style={{ width: size, height: size, ...(css ? { background: css } : {}) }}
    >
      {fill.kind === "default" ? "A" : null}
      {fill.kind === "image" ? picture ? <img src={picture} alt="" className="size-full object-cover" /> : "▣" : null}
    </span>
  );
}

function FillPanel({
  fill,
  modes,
  onChange,
  swatches,
  variables,
}: {
  fill: Fill;
  modes: readonly FillMode[];
  onChange: (fill: Fill, commit?: boolean) => void;
  swatches: Swatches;
  variables?: readonly VariableChoice[];
}) {
  return (
    <div className="flex flex-col gap-3">
      {modes.length > 1 ? (
        <Segmented
          size="sm"
          label="Fill"
          value={fill.kind}
          onChange={(mode) => onChange(switchMode(fill, mode))}
          options={modes.map((mode) => ({ value: mode, label: FILL_MODE_LABELS[mode] }))}
          className="flex w-full [&>button]:flex-1"
        />
      ) : null}

      {fill.kind === "default" ? <p className="m-0 text-[12px] text-muted">The deck's own look, which follows the tablet's theme.</p> : null}
      {fill.kind === "transparent" ? <p className="m-0 text-[12px] text-muted">No face at all: the page's background shows through this key.</p> : null}
      {fill.kind === "solid" ? <SolidEditor color={fill.color} swatches={swatches} onChange={(color, commit) => onChange({ kind: "solid", color }, commit)} /> : null}
      {fill.kind === "gradient" ? <GradientEditor fill={fill} swatches={swatches} onChange={onChange} /> : null}
      {fill.kind === "image" ? <ImageEditor fill={fill} variables={variables} onChange={onChange} /> : null}
    </div>
  );
}

/** The square, the hue, the hex, the swatches: one colour. */
function SolidEditor({ color, swatches, onChange }: { color: string; swatches: Swatches; onChange: (color: string, commit: boolean) => void }) {
  // Kept as HSV so a grey or black does not forget the hue it was dragged from.
  const [hsv, setHsv] = useState<Hsv>(() => hexToHsv(color));
  useEffect(() => {
    if (hsvToHex(hsv) !== color) setHsv(hexToHsv(color));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [color]);
  const set = (next: Hsv, commit: boolean) => {
    setHsv(next);
    onChange(hsvToHex(next), commit);
  };
  const [typed, setTyped] = useState(color);
  useEffect(() => setTyped(color), [color]);
  const recent = recentColors().filter((c) => c !== color && !swatches.some((s) => s.color === c));
  const eyeDropper = typeof window !== "undefined" && "EyeDropper" in window;

  return (
    <div className="flex flex-col gap-2.5">
      <ColorArea hue={hsv.h} saturation={hsv.s} value={hsv.v} onChange={({ saturation, value }, commit) => set({ ...hsv, s: saturation, v: value }, commit)} />
      <HueStrip hue={hsv.h} onChange={(h, commit) => set({ ...hsv, h }, commit)} />
      <div className="flex items-center gap-2">
        <span aria-hidden="true" className="size-8 shrink-0 rounded-md border border-line" style={{ background: color }} />
        <label className="flex h-8 min-w-0 flex-1 items-center gap-1 rounded-[9px] border border-line bg-well px-2 shadow-well focus-within:outline-2 focus-within:outline-signal">
          <span className="font-mono text-[12px] text-faint">#</span>
          <input
            aria-label="Hex colour"
            value={typed.replace(/^#/, "")}
            spellCheck={false}
            maxLength={7}
            onChange={(event) => setTyped(event.target.value)}
            onBlur={() => {
              const parsed = parseHex(typed);
              if (parsed) onChange(parsed, true);
              else setTyped(color);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") (event.target as HTMLInputElement).blur();
            }}
            className="min-w-0 flex-1 bg-transparent font-mono text-[12px] text-ink outline-none"
          />
        </label>
        {eyeDropper ? (
          <button
            type="button"
            title="Pick a colour from the screen"
            onClick={() => {
              const Dropper = (window as unknown as { EyeDropper: new () => { open: () => Promise<{ sRGBHex: string }> } }).EyeDropper;
              void new Dropper()
                .open()
                .then((picked) => {
                  const parsed = parseHex(picked.sRGBHex);
                  if (parsed) onChange(parsed, true);
                })
                .catch(() => {});
            }}
            className="h-8 cursor-pointer rounded-[9px] border border-line px-2.5 text-[11px] text-muted hover:border-line-strong hover:text-ink"
          >
            Pick
          </button>
        ) : null}
      </div>
      <SwatchRow name="Deck" colors={swatches} chosen={color} onPick={(c) => onChange(c, true)} />
      {recent.length > 0 ? <SwatchRow name="Recent" colors={recent.map((c) => ({ color: c, name: c }))} chosen={color} onPick={(c) => onChange(c, true)} /> : null}
    </div>
  );
}

function SwatchRow({ name, colors, chosen, onPick }: { name: string; colors: Swatches; chosen: string; onPick: (color: string) => void }) {
  return (
    <div role="group" aria-label={name} className="flex items-start gap-2">
      <span className="w-11 shrink-0 pt-1 text-[11px] text-faint">{name}</span>
      <div className="flex flex-wrap gap-1.5">
        {colors.map((swatch) => {
          const on = swatch.color === chosen;
          return (
            <button
              key={swatch.color}
              type="button"
              title={swatch.name}
              aria-label={swatch.name}
              aria-pressed={on}
              onClick={() => onPick(swatch.color)}
              className={cn("size-[22px] cursor-pointer rounded-md border", on ? "border-signal ring-2 ring-signal/25" : "border-line hover:border-line-strong")}
              style={{ background: swatch.color }}
            />
          );
        })}
      </div>
    </div>
  );
}

function GradientEditor({ fill, swatches, onChange }: { fill: Extract<Fill, { kind: "gradient" }>; swatches: Swatches; onChange: (fill: Fill, commit?: boolean) => void }) {
  const [stop, setStop] = useState<"from" | "to">("from");
  const css = fillCss(fill) ?? undefined;
  const stopButton = (which: "from" | "to") => (
    <button
      type="button"
      aria-label={which === "from" ? "First colour" : "Second colour"}
      aria-pressed={stop === which}
      onClick={() => setStop(which)}
      className={cn(
        "absolute top-1/2 size-6 -translate-y-1/2 cursor-pointer rounded-full border-2 shadow-[0_0_0_1px_rgb(0_0_0/0.45)]",
        which === "from" ? "left-1.5" : "right-1.5",
        stop === which ? "scale-110 border-white" : "border-white/50",
      )}
      style={{ background: fill[which] }}
    />
  );
  return (
    <div className="flex flex-col gap-3">
      <div className="relative h-10 rounded-lg border border-line" style={{ background: `linear-gradient(90deg, ${fill.from}, ${fill.to})` }}>
        {stopButton("from")}
        {stopButton("to")}
      </div>
      <SolidEditor color={fill[stop]} swatches={swatches} onChange={(color, commit) => onChange({ ...fill, [stop]: color }, commit)} />
      <div className="flex items-center gap-2.5">
        <div className="flex flex-col gap-2">
          <Segmented
            size="sm"
            label="Shape"
            value={fill.radial ? "radial" : "linear"}
            onChange={(shape) => onChange({ ...fill, radial: shape === "radial" })}
            options={[
              { value: "linear", label: "Line" },
              { value: "radial", label: "Round" },
            ]}
          />
          <button type="button" onClick={() => onChange({ ...fill, from: fill.to, to: fill.from })} className="cursor-pointer self-start text-[11px] text-muted hover:text-ink">
            ⇄ Swap colours
          </button>
        </div>
        <span className="ml-auto flex items-center gap-2">
          {fill.radial ? null : (
            <>
              <span className="font-mono text-[11px] text-muted tabular-nums">{fill.angle}°</span>
              <AngleDial angle={fill.angle} onChange={(angle, commit) => onChange({ ...fill, angle }, commit)} label="Fade direction" />
            </>
          )}
          <span aria-hidden="true" className="size-[52px] rounded-lg border border-line" style={{ background: css }} />
        </span>
      </div>
      <div role="group" aria-label="Ready-made fades" className="flex flex-wrap gap-1.5">
        {GRADIENT_PRESETS.map((preset) => {
          const as: Fill = { kind: "gradient", from: preset.from, to: preset.to, angle: preset.angle, radial: preset.radial ?? false };
          return (
            <button
              key={preset.name}
              type="button"
              title={preset.name}
              aria-label={preset.name}
              onClick={() => onChange(as)}
              className="size-7 cursor-pointer rounded-md border border-line hover:border-line-strong"
              style={{ background: fillCss(as) ?? undefined }}
            />
          );
        })}
      </div>
    </div>
  );
}

function ImageEditor({ fill, variables, onChange }: { fill: Extract<Fill, { kind: "image" }>; variables?: readonly VariableChoice[]; onChange: (fill: Fill, commit?: boolean) => void }) {
  const [dim, setDim] = useState(fill.dim);
  useEffect(() => setDim(fill.dim), [fill.dim]);
  return (
    <div className="flex flex-col gap-3">
      <PicturePicker value={fill.image} onPick={(image) => onChange({ ...fill, image })} />
      {variables ? (
        <ValueInput
          label="Picture from a value"
          value={fill.image.startsWith("img:") ? "" : fill.image}
          variables={variables}
          kinds={["text"]}
          placeholder="or follow a value: {{media.current.art}}"
          onCommit={(image) => onChange({ ...fill, image: image.trim() })}
        />
      ) : null}
      <div className="flex items-center gap-2">
        <Segmented
          size="sm"
          label="Picture fit"
          value={fill.fit}
          onChange={(fit: ImageFit) => onChange({ ...fill, fit })}
          options={[
            { value: "IMAGE_FIT_COVER", label: "Fill" },
            { value: "IMAGE_FIT_CONTAIN", label: "Whole" },
          ]}
        />
        <span className="ml-auto text-[12px] text-muted">Darken</span>
        <input
          aria-label="Darken"
          type="range"
          min={0}
          max={90}
          step={5}
          value={dim}
          onChange={(event) => setDim(Number(event.target.value))}
          onPointerUp={() => onChange({ ...fill, dim })}
          onKeyUp={() => onChange({ ...fill, dim })}
          className="w-24 accent-[var(--color-signal)]"
        />
      </div>
    </div>
  );
}

// ---- Recent colours: this browser's, a convenience only ---------------------

const RECENT_KEY = "fd.fill.recent";

function recentColors(): string[] {
  try {
    const stored: unknown = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(stored) ? stored.filter((c): c is string => typeof c === "string" && parseHex(c) === c).slice(0, 8) : [];
  } catch {
    return [];
  }
}

function rememberColor(color: string) {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify([color, ...recentColors().filter((c) => c !== color)].slice(0, 8)));
  } catch {
    // Private windows and blocked storage: there are just no recents.
  }
}
