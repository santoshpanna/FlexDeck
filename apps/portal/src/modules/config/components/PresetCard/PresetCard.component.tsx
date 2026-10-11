import type { ReactNode } from "react";

import { cn } from "@flexdeck/ui";

import type { Gesture, WidgetPreview, WireWidget } from "../../../../services/api/api.ts";
import { presetHalves, type Preset } from "../../lib/presets.lib.ts";
import { FaceView } from "../FaceView/FaceView.component.tsx";

/** A card's key is drawn this many pixels a cell, at the tablet's unit. */
const CELL = 92;

const GESTURE_NAMES: Partial<Record<Gesture, string>> = {
  GESTURE_SHORT_PRESS: "tap",
  GESTURE_DOUBLE_TAP: "double tap",
  GESTURE_LONG_PRESS: "long press",
  GESTURE_HOLD_START: "hold",
};

/** What a preset fills in, in a few words: "live · tap · long press". */
export function presetSummary(preset: Preset): string {
  const { live, gestures } = presetHalves(preset);
  return [...(live ? ["live"] : []), ...gestures.flatMap((gesture) => (GESTURE_NAMES[gesture] ? [GESTURE_NAMES[gesture]] : []))].join(" · ");
}

/** A ready-made key, drawn live as the tablet would, with its name and what it fills in. */
export function PresetCard({
  preset,
  widget,
  preview,
  onClick,
  chosen = false,
  after,
}: {
  preset: Preset;
  /** The preset as a widget with an id, for the live preview. */
  widget: WireWidget;
  preview: WidgetPreview | undefined;
  onClick: () => void;
  /** It is the one just put on the key. */
  chosen?: boolean;
  /** Small follow-ups under it, outside its button. */
  after?: ReactNode;
}) {
  const w = preset.w * CELL;
  const h = preset.h * CELL;
  const scale = Math.min(1, (preset.w > 1 ? 220 : 104) / w, 80 / h);
  const summary = presetSummary(preset);
  return (
    <div
      className={cn(
        "flex flex-col rounded-xl border bg-well/50 transition-colors",
        chosen ? "border-signal" : "border-line hover:border-line-strong hover:bg-plate-2",
        preset.w > 1 && "col-span-2",
      )}
    >
    <button
      type="button"
      title={preset.description}
      aria-label={`Use ${preset.name}: ${preset.description}`}
      onClick={onClick}
      aria-pressed={chosen}
      className="group flex cursor-pointer flex-col gap-2 rounded-xl p-2.5 text-left"
    >
      <span className="flex h-[84px] items-center justify-center">
        <span className="relative" style={{ width: w * scale, height: h * scale }}>
          <span
            className="absolute top-0 left-0 overflow-hidden rounded-[14px] border border-[var(--fd-deck-key-line)] bg-linear-to-b from-[var(--fd-deck-key-top)] to-[var(--fd-deck-key-bottom)] text-[var(--fd-deck-ink)] shadow-key"
            style={{ width: w, height: h, transform: `scale(${scale})`, transformOrigin: "top left" }}
          >
            <FaceView widget={widget} w={w} h={h} unit={0.72} radius={14} preview={preview} />
          </span>
        </span>
      </span>
      <span className="flex min-w-0 flex-col gap-0.5">
        <span className="truncate text-[12px] font-semibold text-ink">{preset.name}</span>
        {summary ? <span className="truncate font-mono text-[10px] tracking-[0.08em] text-faint">{summary}</span> : null}
      </span>
    </button>
    {after ? <div className="flex flex-wrap gap-x-3 gap-y-1 px-2.5 pb-2.5 text-[11px]">{after}</div> : null}
    </div>
  );
}
