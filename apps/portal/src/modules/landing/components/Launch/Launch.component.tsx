import { useMotionValueEvent, useScroll } from "motion/react";
import { useCallback, useRef, useState } from "react";
import { Link } from "react-router-dom";

import { cn, keyVariants } from "@flexdeck/ui";
import { AsciiScene, RocketSim } from "@flexdeck/ui/ascii";

import { AppLink } from "../../../../common/components/AppLink/AppLink.component.tsx";
import { latest } from "../../../../common/release/release.ts";
import { ASSETS } from "../../../../common/release/release.lib.ts";
import type { LandingCallToAction } from "../../view/landing.view.tsx";

/** Straight to the file once there is a release; before that, the download page explains. */
const download = (url: string) => (latest ? url : "/download");

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
          <div className="flex flex-wrap justify-center gap-3">
            <a href={download(ASSETS.windows.url)} className={cn(keyVariants({ tone: "signal", size: "lg" }), "h-14 px-7 no-underline")}>
              Download for Windows
            </a>
            <a href={download(ASSETS.android.url)} className={cn(keyVariants({ size: "lg" }), "h-14 px-6 no-underline")}>
              Get the Android app
            </a>
            <AppLink to={callToAction.to} className={cn(keyVariants({ size: "lg" }), "h-14 px-6 no-underline")}>
              {callToAction.label}
            </AppLink>
          </div>
          <span className="font-mono text-[10px] tracking-[0.18em] text-muted uppercase">
            Windows 10 / 11 · Android 5.0+ ·{" "}
            <Link to="/download" className="text-signal no-underline hover:text-signal-hi">
              All files and checksums →
            </Link>
          </span>
        </div>
        <div className="relative flex flex-wrap justify-center gap-7 rounded-full bg-body/80 px-4 py-1.5 font-mono text-[10px] tracking-[0.2em] text-faint uppercase backdrop-blur-sm">
          <Link to="/changelog" className="text-faint no-underline hover:text-ink">
            {latest ? `${latest.version} · Changelog` : "Changelog"}
          </Link>
          <Link to="/docs" className="text-faint no-underline hover:text-ink">
            Docs
          </Link>
          <span>AGPL-3.0</span>
          <span>Made for tablets in drawers</span>
        </div>
      </div>
    </section>
  );
}
