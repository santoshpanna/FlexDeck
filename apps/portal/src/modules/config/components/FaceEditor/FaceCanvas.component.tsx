import { boxOf, placeBox, type Face, type FaceElement, type Rect } from "@flexdeck/face";
import { useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

import { cn } from "@flexdeck/ui";

import type { WidgetPreview, WireWidget } from "../../../../services/api/api.ts";
import { NUDGE, canvasGeometry, moveBox } from "../../lib/canvas.lib.ts";
import { FaceView } from "../FaceView/FaceView.component.tsx";

const KIND_NAMES: Record<string, string> = {
  ELEMENT_KIND_TEXT: "Text",
  ELEMENT_KIND_ICON: "Icon",
  ELEMENT_KIND_BAR: "Bar",
  ELEMENT_KIND_GAUGE: "Gauge",
  ELEMENT_KIND_GRAPH: "Graph",
  ELEMENT_KIND_CLOCK: "Clock",
  ELEMENT_KIND_TIMER: "Timer",
};

/**
 * The key as the tablet draws it, with every element outlined: drag one to
 * put it anywhere (one still in its layout's place comes free as it moves),
 * drag its corner to size it. Moves snap to a fine grid unless Alt is held;
 * arrow keys nudge, Shift+arrows size. A change is kept when the finger
 * lifts, so a drag is one step to undo.
 */
export function FaceCanvas({
  widget,
  face,
  lookId,
  preview,
  selected,
  onSelect,
  onBox,
}: {
  widget: WireWidget;
  face: Face;
  lookId?: string;
  preview?: WidgetPreview;
  selected: number | null;
  onSelect: (index: number) => void;
  onBox: (index: number, box: Rect | undefined) => void;
}) {
  const { w, h, faceH, rects } = canvasGeometry(widget, face);
  const scale = Math.min(300 / w, 240 / h);
  const elements: FaceElement[] = face.elements ?? [];
  const [moving, setMoving] = useState<{ index: number; box: Rect } | null>(null);
  const drag = useRef<{ index: number; x: number; y: number; start: Rect; resize: boolean; moved: boolean; box?: Rect } | null>(null);

  const boxFor = (index: number): Rect => elements[index]?.box ?? boxOf(rects[index]!, w, faceH);

  const begin = (event: PointerEvent, index: number, resize: boolean) => {
    event.preventDefault();
    event.stopPropagation();
    onSelect(index);
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    drag.current = { index, x: event.clientX, y: event.clientY, start: boxFor(index), resize, moved: false };
  };
  const move = (event: PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const dx = (event.clientX - d.x) / scale / w;
    const dy = (event.clientY - d.y) / scale / faceH;
    if (!d.moved && Math.abs(event.clientX - d.x) + Math.abs(event.clientY - d.y) < 3) return;
    d.moved = true;
    d.box = moveBox(d.start, dx, dy, d.resize, event.altKey);
    setMoving({ index: d.index, box: d.box });
  };
  const end = () => {
    const d = drag.current;
    drag.current = null;
    if (d?.moved && d.box) onBox(d.index, d.box);
    setMoving(null);
  };
  const keys = (event: KeyboardEvent, index: number) => {
    const step = { ArrowLeft: [-NUDGE, 0], ArrowRight: [NUDGE, 0], ArrowUp: [0, -NUDGE], ArrowDown: [0, NUDGE] }[event.key];
    if (!step) return;
    event.preventDefault();
    onBox(index, moveBox(boxFor(index), step[0]!, step[1]!, event.shiftKey, true));
  };

  return (
    <div className="flex flex-col items-center gap-2">
      {/* The key's own edge and middle, so a piece can be put by eye. */}
      <div className="relative" style={{ width: w * scale, height: h * scale, borderRadius: 18 * scale, boxShadow: "0 0 0 1px var(--color-line-strong), 0 0 0 5px var(--color-well)" }}>
        <div
          className="absolute top-0 left-0 overflow-visible rounded-[18px] bg-linear-to-b from-[var(--fd-deck-key-top)] to-[var(--fd-deck-key-bottom)] text-[var(--fd-deck-ink)]"
          style={{ width: w, height: h, transform: `scale(${scale})`, transformOrigin: "0 0" }}
        >
          <FaceView widget={widget} lookId={lookId} w={w} h={h} unit={1} radius={18} preview={preview} />
          <span aria-hidden="true" className="pointer-events-none absolute inset-y-0 left-1/2 border-l border-dashed border-white/10" />
          <span aria-hidden="true" className="pointer-events-none absolute inset-x-0 border-t border-dashed border-white/10" style={{ top: faceH / 2 }} />
          {elements.map((element, index) => {
            const rect = moving?.index === index ? placeBox(moving.box, w, faceH) : rects[index]!;
            const on = selected === index;
            const free = Boolean(element.box) || moving?.index === index;
            const name = KIND_NAMES[element.kind ?? "ELEMENT_KIND_TEXT"] ?? "Element";
            return (
              <div
                key={index}
                role="button"
                tabIndex={0}
                aria-label={`${name} ${index + 1}: drag to move${free ? ", arrow keys to nudge, Shift+arrows to size" : ""}`}
                aria-pressed={on}
                onPointerDown={(event) => begin(event, index, false)}
                onPointerMove={move}
                onPointerUp={end}
                onPointerCancel={end}
                onKeyDown={(event) => keys(event, index)}
                onFocus={() => onSelect(index)}
                className={cn(
                  "absolute cursor-move touch-none outline-none",
                  on ? "border-2 border-signal" : free ? "border border-white/50 hover:border-signal" : "border border-dashed border-white/30 hover:border-signal",
                )}
                style={{ left: rect.x, top: rect.y, width: Math.max(rect.w, 6), height: Math.max(rect.h, 6), borderRadius: 4, zIndex: on ? 2 : 1 }}
              >
                {on ? (
                  <>
                    <span className="pointer-events-none absolute -top-5 left-0 rounded bg-signal px-1 text-[10px] leading-4 whitespace-nowrap text-white">
                      {name}
                      {free ? "" : " · in its place"}
                    </span>
                    <span
                      aria-hidden="true"
                      onPointerDown={(event) => begin(event, index, true)}
                      className="absolute -right-2 -bottom-2 size-4 cursor-nwse-resize rounded-full border-2 border-white bg-signal"
                    />
                  </>
                ) : null}
              </div>
            );
          })}
        </div>
      </div>
      <p className="m-0 text-center text-[11px] text-faint">
        Drag a piece to put it anywhere; drag its corner to size it. Alt: no snapping. Arrows nudge, Shift+arrows size.
      </p>
      {selected !== null && elements[selected]?.box ? (
        <button type="button" onClick={() => onBox(selected, undefined)} className="text-[12px] text-signal hover:underline">
          Put it back in its place
        </button>
      ) : null}
    </div>
  );
}
