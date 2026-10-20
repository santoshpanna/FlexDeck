import { motion, useReducedMotion } from "motion/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { AppLink } from "../../../../common/components/AppLink/AppLink.component.tsx";

import { ease } from "@flexdeck/ui";
import { AsciiScene, decode, decodeDuration, render } from "@flexdeck/ui/ascii";

import type { LandingCallToAction } from "../../view/landing.view.tsx";

/** The headline resolving out of noise, settling on the real words. */
function useDecoded(text: string): string {
  const reduce = useReducedMotion();
  const [shown, setShown] = useState(() => (reduce || typeof window === "undefined" ? text : decode(text, 0)));
  useEffect(() => {
    if (reduce) {
      setShown(text);
      return;
    }
    const start = performance.now();
    const end = decodeDuration(text);
    let raf = 0;
    const tick = (now: number) => {
      const t = (now - start) / 1000;
      setShown(decode(text, t));
      if (t <= end) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [text, reduce]);
  return shown;
}

/**
 * The first screen: what this is in one line, and the deck itself, drawn in
 * characters by the ray-marcher, slowly turning and tilting toward the cursor
 * while its keys flash as if someone were playing it.
 */
export function Hero({ headline, blurb, callToAction, facts }: { headline: string; blurb: string; callToAction: LandingCallToAction; facts: string[] }) {
  const title = useDecoded(headline);
  const pointer = useRef({ x: 0, y: 0, cx: 0, cy: 0 });
  const lit = useRef<number[]>(Array(15).fill(0));
  const [yaw, setYaw] = useState(35);

  useEffect(() => {
    const move = (event: PointerEvent) => {
      pointer.current.x = (event.clientX / window.innerWidth - 0.5) * 2;
      pointer.current.y = (event.clientY / window.innerHeight - 0.5) * 2;
    };
    window.addEventListener("pointermove", move);
    return () => window.removeEventListener("pointermove", move);
  }, []);

  const frames = useRef(0);
  const draw = useCallback((t: number) => {
    const p = pointer.current;
    p.cx += (p.x - p.cx) * 0.08;
    p.cy += (p.y - p.cy) * 0.08;
    if (Math.random() < 0.2) lit.current[Math.floor(Math.random() * 15)] = 1;
    lit.current = lit.current.map((value) => value * 0.9);
    const turn = 0.62 + Math.sin(t * 0.33) * 0.38 + p.cx * 0.7;
    frames.current += 1;
    if (frames.current % 6 === 0) setYaw(Math.round(turn * 57.3));
    return render({ cols: 132, rows: 56, yaw: turn, pitch: -p.cy * 0.25, elev: 0.78, dist: 5, fl: 3.3, t, lit: lit.current, reveal: Math.min(1, t / 1.8) });
  }, []);

  return (
    <header className="relative mx-auto flex min-h-[calc(100vh-73px)] max-w-[1440px] flex-wrap items-center gap-8 overflow-hidden px-[clamp(16px,4vw,48px)] py-10">
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.8, ease: ease.outExpo }}
        className="relative z-10 flex min-w-0 flex-[1_1_380px] flex-col gap-6"
      >
        <span className="lbl text-signal">// a control deck for any tablet</span>
        <h1 aria-label={headline} className="m-0 font-mono text-[clamp(44px,5.4vw,84px)] leading-[0.98] font-medium tracking-[-0.03em] whitespace-pre-line">
          {title}
        </h1>
        <p className="m-0 max-w-[460px] text-lg leading-relaxed text-ink-2">{blurb}</p>
        <div className="flex flex-wrap gap-3">
          <AppLink
            to={callToAction.to}
            className="inline-flex h-13 items-center rounded-key border border-signal-hi bg-linear-to-b from-signal-hi to-signal px-6 text-[15px] font-semibold text-signal-ink no-underline shadow-key transition-transform active:translate-y-0.5"
          >
            {callToAction.label}
          </AppLink>
          <a
            href="#anatomy"
            className="inline-flex h-13 items-center rounded-key border border-[var(--fd-key-line)] bg-linear-to-b from-[var(--fd-key-top)] to-[var(--fd-key-bottom)] px-5 text-[15px] text-ink no-underline shadow-key transition-transform active:translate-y-0.5"
          >
            See it come apart ↓
          </a>
        </div>
        <div className="mt-2 flex flex-wrap gap-6 font-mono text-[10px] tracking-[0.2em] text-faint uppercase">
          {facts.map((fact) => (
            <span key={fact}>{fact}</span>
          ))}
        </div>
      </motion.div>
      <div className="relative flex min-w-0 flex-[1.4_1_560px] items-center justify-center">
        <AsciiScene draw={draw} fps={28} size={9} tone="text-ink-2" still={3} />
        <span className="absolute right-0 -bottom-2 font-mono text-[10px] tracking-[0.18em] text-faint">
          FIG.01 — DECK · YAW {String(yaw).padStart(3, "0")}° · MOVE YOUR CURSOR
        </span>
      </div>
    </header>
  );
}
