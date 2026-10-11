import { useRef, type KeyboardEvent, type PointerEvent } from "react";

import { cn } from "../../lib/cn.ts";

/** `commit`: the drag ended, or a key was pressed — the moment to keep it. */
export type DragChange<T> = (value: T, commit: boolean) => void;

/**
 * Follow a pointer over `element` as fractions 0–1 across and down, from the
 * press through the release. The release is the commit.
 */
function usePointerFraction(onMove: (x: number, y: number, commit: boolean) => void) {
  const area = useRef<HTMLDivElement>(null);
  const read = (event: PointerEvent, commit: boolean) => {
    const box = area.current?.getBoundingClientRect();
    if (!box || box.width === 0 || box.height === 0) return;
    const x = Math.min(1, Math.max(0, (event.clientX - box.left) / box.width));
    const y = Math.min(1, Math.max(0, (event.clientY - box.top) / box.height));
    onMove(x, y, commit);
  };
  return {
    ref: area,
    onPointerDown: (event: PointerEvent) => {
      event.currentTarget.setPointerCapture(event.pointerId);
      read(event, false);
    },
    onPointerMove: (event: PointerEvent) => {
      if (event.currentTarget.hasPointerCapture(event.pointerId)) read(event, false);
    },
    onPointerUp: (event: PointerEvent) => {
      if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
      read(event, true);
      event.currentTarget.releasePointerCapture(event.pointerId);
    },
  };
}

/** Arrow keys nudge by a step, Shift by ten. */
function nudge(event: KeyboardEvent, step: number): [number, number] | null {
  const by = event.shiftKey ? step * 10 : step;
  switch (event.key) {
    case "ArrowLeft":
      return [-by, 0];
    case "ArrowRight":
      return [by, 0];
    case "ArrowUp":
      return [0, -by];
    case "ArrowDown":
      return [0, by];
    default:
      return null;
  }
}

const clamp = (n: number) => Math.min(1, Math.max(0, n));

/**
 * Saturation across, brightness up: the square of a colour picker, for one
 * hue. Values are fractions 0–1.
 */
export function ColorArea({
  hue,
  saturation,
  value,
  onChange,
  label = "Colour",
  className,
}: {
  hue: number;
  saturation: number;
  value: number;
  onChange: DragChange<{ saturation: number; value: number }>;
  label?: string;
  className?: string;
}) {
  const pointer = usePointerFraction((x, y, commit) => onChange({ saturation: x, value: 1 - y }, commit));
  return (
    <div
      {...pointer}
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuetext={`saturation ${Math.round(saturation * 100)}%, brightness ${Math.round(value * 100)}%`}
      onKeyDown={(event) => {
        const move = nudge(event, 0.01);
        if (!move) return;
        event.preventDefault();
        onChange({ saturation: clamp(saturation + move[0]), value: clamp(value - move[1]) }, true);
      }}
      className={cn("relative h-36 w-full cursor-crosshair touch-none rounded-lg border border-line select-none", className)}
      style={{
        background: `linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, hsl(${hue} 100% 50%))`,
      }}
    >
      <span
        aria-hidden="true"
        className="pointer-events-none absolute size-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow-[0_0_0_1px_rgb(0_0_0/0.5)]"
        style={{ left: `${saturation * 100}%`, top: `${(1 - value) * 100}%` }}
      />
    </div>
  );
}

/** The rainbow strip that picks a hue, 0–360. */
export function HueStrip({ hue, onChange, label = "Hue", className }: { hue: number; onChange: DragChange<number>; label?: string; className?: string }) {
  const pointer = usePointerFraction((x, _y, commit) => onChange(x * 360, commit));
  return (
    <div
      {...pointer}
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={360}
      aria-valuenow={Math.round(hue)}
      onKeyDown={(event) => {
        const move = nudge(event, 1);
        if (!move) return;
        event.preventDefault();
        onChange(Math.min(360, Math.max(0, hue + move[0] - move[1])), true);
      }}
      className={cn("relative h-3.5 w-full cursor-pointer touch-none rounded-full border border-line select-none", className)}
      style={{ background: "linear-gradient(to right, #f00, #ff0, #0f0, #0ff, #00f, #f0f, #f00)" }}
    >
      <span
        aria-hidden="true"
        className="pointer-events-none absolute top-1/2 size-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow-[0_0_0_1px_rgb(0_0_0/0.5)]"
        style={{ left: `${(hue / 360) * 100}%`, background: `hsl(${hue} 100% 50%)` }}
      />
    </div>
  );
}

/**
 * A knob for a direction in degrees, as CSS has it: 0 points up, 90 right.
 * Drag round it; Shift snaps to 45°.
 */
export function AngleDial({ angle, onChange, label = "Angle", size = 52 }: { angle: number; onChange: DragChange<number>; label?: string; size?: number }) {
  const dial = useRef<HTMLDivElement>(null);
  const read = (event: PointerEvent, commit: boolean) => {
    const box = dial.current?.getBoundingClientRect();
    if (!box) return;
    const dx = event.clientX - (box.left + box.width / 2);
    const dy = event.clientY - (box.top + box.height / 2);
    let degrees = ((Math.atan2(dx, -dy) * 180) / Math.PI + 360) % 360;
    degrees = event.shiftKey ? Math.round(degrees / 45) * 45 : Math.round(degrees);
    onChange(degrees % 360, commit);
  };
  return (
    <div
      ref={dial}
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={359}
      aria-valuenow={angle}
      aria-valuetext={`${angle} degrees`}
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId);
        read(event, false);
      }}
      onPointerMove={(event) => {
        if (event.currentTarget.hasPointerCapture(event.pointerId)) read(event, false);
      }}
      onPointerUp={(event) => {
        if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
        read(event, true);
        event.currentTarget.releasePointerCapture(event.pointerId);
      }}
      onKeyDown={(event) => {
        const move = nudge(event, 1);
        if (!move) return;
        event.preventDefault();
        onChange((((angle + move[0] - move[1]) % 360) + 360) % 360, true);
      }}
      className="relative shrink-0 cursor-grab touch-none rounded-full border border-line bg-well shadow-well select-none active:cursor-grabbing"
      style={{ width: size, height: size }}
    >
      {Array.from({ length: 8 }, (_, tick) => (
        <span key={tick} aria-hidden="true" className="absolute inset-0" style={{ transform: `rotate(${tick * 45}deg)` }}>
          <span className="absolute top-[3px] left-1/2 -ml-[1.5px] size-[3px] rounded-full bg-faint" />
        </span>
      ))}
      <span aria-hidden="true" className="absolute inset-[22%] rounded-full bg-ink shadow-[0_2px_0_var(--fd-key-base)]" />
      <span aria-hidden="true" className="absolute inset-0" style={{ transform: `rotate(${angle}deg)` }}>
        <span className="absolute top-[26%] left-1/2 -ml-[1.5px] h-[18%] w-[3px] rounded-full bg-signal" />
      </span>
    </div>
  );
}
