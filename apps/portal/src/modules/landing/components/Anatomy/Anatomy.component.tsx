import { AnimatePresence, motion, useMotionValueEvent, useScroll } from "motion/react";
import { useCallback, useRef, useState } from "react";

import { cn } from "@flexdeck/ui";
import { AsciiScene, hash, project, render, type SceneOptions } from "@flexdeck/ui/ascii";

import type { LandingCallout } from "../../view/landing.view.tsx";

const COLS = 156;
const ROWS = 62;

/** The scene at a scroll position: from a three-quarter view to overhead, keycaps lifting off. */
function sceneAt(p: number, t: number): SceneOptions {
  const e0 = p < 0.12 ? 0 : p > 0.7 ? 1 : (p - 0.12) / 0.58;
  const e = e0 * e0 * (3 - 2 * e0);
  const lit = Array.from({ length: 15 }, (_, k) => (k === 7 || (p > 0.55 && (k === 2 || k === 12)) ? 1 : 0));
  return { cols: COLS, rows: ROWS, yaw: 0.55 - e * 0.45 + Math.sin(t * 0.3) * 0.05, elev: 0.6 + e * 0.45, dist: 5.4 + e * 1.8, fl: 3.1, t, explode: e, lit, lookY: e * 0.4 };
}

/** Where each callout points: a key, the lit key, the plate's corner, its underside. */
function anchors(e: number): [number, number, number][] {
  const lift = (k: number) => 0.2 + e * (0.45 + 1.25 * hash(k + 3));
  return [
    [-0.56, lift(1), -0.56],
    [0, lift(7), 0],
    [1.62, -e * 0.35, -1.02],
    [-1.62, -0.16 - e * 0.35, 1.02],
  ];
}

const THRESHOLDS = [0.25, 0.4, 0.55, 0.7];

/**
 * Where each label sits, as a fixed slot beside the figure: two per side, one
 * high and one low, on the side its part is on. Fixed so two labels can never
 * land on top of each other however the model turns; the leader line does the
 * pointing.
 */
const SLOTS: readonly { side: "left" | "right"; y: number }[] = [
  { side: "left", y: 24 },
  { side: "right", y: 24 },
  { side: "right", y: 72 },
  { side: "left", y: 72 },
];

/**
 * The deck, pulled apart by scrolling: the section pins while the keycaps lift
 * off and the camera swings overhead, and a label with a leader line lands on
 * each part as it separates — the technical-poster treatment.
 */
export function Anatomy({ callouts }: { callouts: LandingCallout[] }) {
  const section = useRef<HTMLElement>(null);
  const { scrollYProgress } = useScroll({ target: section, offset: ["start start", "end end"] });
  const progress = useRef(0);
  const [p, setP] = useState(0);

  useMotionValueEvent(scrollYProgress, "change", (value) => {
    progress.current = value;
    setP(Math.round(value * 100) / 100);
  });

  const draw = useCallback((t: number) => render(sceneAt(progress.current, t)), []);

  const scene = sceneAt(p, 0);
  const points = anchors(scene.explode ?? 0).map(([x, y, z]) => project(scene, x, y, z));

  return (
    <section id="anatomy" ref={section} className="relative h-[300vh]">
      <div className="sticky top-[73px] flex h-[calc(100vh-73px)] flex-col overflow-hidden px-[clamp(16px,4vw,48px)] py-7">
        <div className="mx-auto flex w-full max-w-[1440px] flex-wrap items-end justify-between gap-3">
          <div>
            <span className="lbl">§ 02 — Anatomy</span>
            <AnimatePresence mode="wait" initial={false}>
              <motion.h2
                key={p < 0.2 ? 0 : p < 0.75 ? 1 : 2}
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -10 }}
                className="m-0 mt-2 text-[clamp(30px,3.4vw,48px)] leading-none font-semibold tracking-[-0.02em]"
              >
                {p < 0.2 ? "One deck. Four layers." : p < 0.75 ? "Pull it apart." : "Every part, yours to shape."}
              </motion.h2>
            </AnimatePresence>
          </div>
          <span className="font-mono text-[11px] tracking-[0.18em] text-muted">FIG.02 — EXPLODED · {String(Math.round(p * 100)).padStart(3, "0")}%</span>
        </div>
        <div className="relative flex min-h-0 flex-1 items-center justify-center">
          <div className="relative">
            <AsciiScene draw={draw} fps={26} size={8} tone="text-ink-2" still={0} />
            {/* Leader lines: from each label's fixed slot, across to a knee
                beside its part, then in to the dot. One SVG over the figure, in
                percent coordinates, so the lines track the model as it moves. */}
            <svg aria-hidden="true" viewBox="0 0 100 100" preserveAspectRatio="none" className="pointer-events-none absolute inset-0 size-full overflow-visible max-xl:hidden">
              {callouts.map((callout, index) => {
                const at = points[index];
                const slot = SLOTS[index];
                if (!at || !slot) return null;
                const x = at.x * 100;
                const y = at.y * 100;
                const edge = slot.side === "left" ? -3 : 103;
                const knee = slot.side === "left" ? Math.min(x - 6, 12) : Math.max(x + 6, 88);
                return (
                  <polyline
                    key={callout.tag}
                    points={`${edge},${slot.y} ${knee},${slot.y} ${x},${y}`}
                    fill="none"
                    stroke="var(--color-muted)"
                    strokeWidth="1"
                    strokeDasharray="4 3"
                    vectorEffect="non-scaling-stroke"
                    className="transition-opacity duration-500"
                    style={{ opacity: p >= (THRESHOLDS[index] ?? 1) ? 1 : 0 }}
                  />
                );
              })}
            </svg>
            {callouts.map((callout, index) => {
              const at = points[index];
              const slot = SLOTS[index];
              if (!at || !slot) return null;
              const on = p >= (THRESHOLDS[index] ?? 1);
              const x = at.x * 100;
              const y = at.y * 100;
              return (
                <div key={callout.tag} aria-hidden={!on}>
                  <span
                    className="absolute size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-signal shadow-[0_0_0_4px_rgb(255_90_31/0.18),0_0_14px_var(--color-signal)] transition-opacity duration-300"
                    style={{ top: `${y}%`, left: `${x}%`, opacity: on ? 1 : 0 }}
                  />
                  <div
                    className={cn(
                      "absolute w-[220px] -translate-y-1/2 transition-[opacity,margin] duration-500 ease-out max-xl:hidden",
                      on ? "mt-0 opacity-100" : "mt-2 opacity-0",
                      slot.side === "left" ? "text-right" : "text-left",
                    )}
                    style={{ top: `${slot.y}%`, ...(slot.side === "left" ? { right: "104%" } : { left: "104%" }) }}
                  >
                    <div className="font-mono text-[10px] tracking-[0.2em] text-signal uppercase">{callout.tag}</div>
                    <div className="mt-1 text-[17px] font-semibold">{callout.title}</div>
                    <div className="mt-1 text-[13px] leading-snug text-muted">{callout.body}</div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
        <div className="mx-auto grid w-full max-w-[1440px] gap-4 sm:grid-cols-2 xl:hidden">
          {callouts.map((callout, index) => (
            <div key={callout.tag} className={cn("transition-opacity duration-500", p >= (THRESHOLDS[index] ?? 1) ? "opacity-100" : "opacity-25")}>
              <div className="font-mono text-[10px] tracking-[0.2em] text-signal uppercase">{callout.tag}</div>
              <div className="text-[15px] font-semibold">{callout.title}</div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
