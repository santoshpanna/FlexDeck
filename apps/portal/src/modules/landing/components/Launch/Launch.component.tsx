import { useMotionValueEvent, useScroll } from "motion/react";
import { useCallback, useRef, useState } from "react";
import { Link } from "react-router-dom";

import { AsciiScene, RocketSim } from "@flexdeck/ui/ascii";

import type { LandingCallToAction } from "../../view/landing.view.tsx";

/**
 * The end: a launch. The section pins, and scrolling through it lifts the rocket
 * off the pad — exhaust hits the ground and rolls out into cloud banks while
 * rings widen behind it.
 */
export function Launch({ callToAction }: { callToAction: LandingCallToAction }) {
  const section = useRef<HTMLElement>(null);
  const { scrollYProgress } = useScroll({ target: section, offset: ["start start", "end end"] });
  const lift = useRef(0);
  const [seconds, setSeconds] = useState(0);
  const sim = useRef<RocketSim | null>(null);

  useMotionValueEvent(scrollYProgress, "change", (value) => {
    lift.current = value;
    setSeconds(Math.round(value * 40));
  });

  const draw = useCallback((t: number) => {
    sim.current ??= new RocketSim(150, 64);
    return sim.current.step(t, lift.current);
  }, []);

  return (
    <section ref={section} className="relative h-[200vh]">
      <div className="sticky top-[73px] flex h-[calc(100vh-73px)] flex-col items-center justify-between overflow-hidden px-[clamp(16px,4vw,48px)] py-8">
        <div className="pointer-events-none absolute inset-0 flex items-end justify-center">
          <AsciiScene draw={draw} fps={28} size={9} tone="text-muted" still={2} />
        </div>
        <div className="relative flex flex-col items-center gap-5 text-center">
          <span className="lbl">§ 06 — {seconds > 1 ? `T+${String(seconds).padStart(2, "0")}s · Liftoff` : "T-00 · Ready"}</span>
          <h2 className="m-0 text-[clamp(40px,5vw,84px)] leading-[0.95] font-semibold tracking-[-0.03em]">Launch your deck.</h2>
          <Link
            to={callToAction.to}
            className="inline-flex h-14 items-center rounded-key border border-signal-hi bg-linear-to-b from-signal-hi to-signal px-7 text-[15px] font-semibold text-signal-ink no-underline shadow-key transition-transform active:translate-y-0.5"
          >
            {callToAction.label}
          </Link>
        </div>
        <div className="relative flex flex-wrap justify-center gap-7 rounded-full bg-body/80 px-4 py-1.5 font-mono text-[10px] tracking-[0.2em] text-faint uppercase backdrop-blur-sm">
          <span>v0.0.1</span>
          <span>MIT license</span>
          <span>Made for tablets in drawers</span>
        </div>
      </div>
    </section>
  );
}
