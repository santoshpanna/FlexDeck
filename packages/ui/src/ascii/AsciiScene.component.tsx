import { useEffect, useRef } from "react";

import { cn } from "../lib/cn.ts";
import type { Frame } from "./engine.ts";

export interface AsciiSceneProps {
  /**
   * Draw the frame for time `t` (seconds since the scene started). With `fit`,
   * `cols` is how many characters fit across the scene's box, right now.
   */
  draw: (t: number, cols?: number) => Frame;
  /** Fill the width it is given: measure it, and hand `draw` the column count. */
  fit?: { min?: number; max?: number };
  /** Frames per second to aim for. ASCII reads as smooth well below 60. */
  fps?: number;
  /** Font size in px; the line height matches it so cells are square-ish. */
  size?: number;
  /** The time drawn once, and held, when motion is reduced. */
  still?: number;
  /** Base ink colour class. */
  tone?: string;
  className?: string;
}

/**
 * Runs an ASCII scene into two stacked `<pre>`s — ink, and the glowing signal
 * layer — writing `textContent` directly from the animation loop. React renders
 * the scene ONCE; frames never go through state, so a 10 000-cell figure costs a
 * string build and two text writes per frame, not a reconciliation.
 *
 * Off-screen or in a hidden tab it stops. With reduced motion it draws a single
 * still frame. It is decoration: hidden from assistive technology, so whatever
 * it illustrates must also be said in text nearby.
 */
export function AsciiScene({ draw, fit, fps = 30, size = 9, still = 4, tone = "text-ink-2", className }: AsciiSceneProps) {
  const host = useRef<HTMLDivElement>(null);
  const ink = useRef<HTMLPreElement>(null);
  const glow = useRef<HTMLPreElement>(null);
  const drawRef = useRef(draw);
  drawRef.current = draw;
  const cols = useRef<number | undefined>(undefined);
  const fitMin = fit?.min ?? 1;
  const fitMax = fit?.max ?? Infinity;
  const fitting = fit !== undefined;

  useEffect(() => {
    let now = still;
    const paint = (t: number) => {
      now = t;
      const frame = drawRef.current(t, cols.current);
      if (ink.current) ink.current.textContent = frame.base;
      if (glow.current) glow.current.textContent = frame.glow;
    };

    // The column count, from the box's width and one character's: measured in
    // the scene's own face, so it is right whatever that font turns out to be.
    let resize: ResizeObserver | null = null;
    if (fitting && host.current && ink.current) {
      const probe = document.createElement("span");
      probe.textContent = "0".repeat(40);
      probe.style.visibility = "hidden";
      probe.style.position = "absolute";
      ink.current.after(probe);
      probe.className = ink.current.className;
      probe.style.fontSize = `${size}px`;
      const cell = probe.getBoundingClientRect().width / 40 || size * 0.6;
      probe.remove();
      const measure = () => {
        const width = host.current?.clientWidth ?? 0;
        const next = Math.max(fitMin, Math.min(fitMax, Math.floor(width / cell)));
        if (next !== cols.current) {
          cols.current = next;
          paint(now);
        }
      };
      measure();
      resize = new ResizeObserver(measure);
      resize.observe(host.current);
    }

    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce) {
      paint(still);
      return () => resize?.disconnect();
    }

    let visible = true;
    let raf = 0;
    let last = 0;
    const start = performance.now();
    const interval = 1000 / fps;
    const loop = (now: number) => {
      raf = requestAnimationFrame(loop);
      if (!visible || document.hidden || now - last < interval) return;
      last = now;
      paint((now - start) / 1000);
    };
    paint(0);
    raf = requestAnimationFrame(loop);

    const observer = new IntersectionObserver(([entry]) => {
      visible = entry?.isIntersecting ?? true;
    });
    if (host.current) observer.observe(host.current);

    return () => {
      cancelAnimationFrame(raf);
      observer.disconnect();
      resize?.disconnect();
    };
  }, [fps, still, fitting, fitMin, fitMax, size]);

  return (
    <div ref={host} aria-hidden="true" className={cn("relative", fit && "w-full overflow-hidden", className)}>
      <pre ref={ink} className={cn("ascii", tone)} style={{ fontSize: size, lineHeight: `${size}px` }} />
      <pre
        ref={glow}
        className="ascii ascii-glow absolute inset-0"
        style={{ fontSize: size, lineHeight: `${size}px` }}
      />
    </div>
  );
}
