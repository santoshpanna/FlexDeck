import { motion, useScroll, useSpring } from "motion/react";
import { Link } from "react-router-dom";

import { ThemeToggle } from "../../../common/components/ThemeToggle/ThemeToggle.component.tsx";
import { Mark } from "@flexdeck/ui";
import type { FieldKind } from "@flexdeck/ui/ascii";
import { Anatomy } from "../components/Anatomy/Anatomy.component.tsx";
import { FeatureGrid } from "../components/FeatureGrid/FeatureGrid.component.tsx";
import { Hero } from "../components/Hero/Hero.component.tsx";
import { Launch } from "../components/Launch/Launch.component.tsx";
import { Marquee } from "../components/Marquee/Marquee.component.tsx";
import { Steps } from "../components/Steps/Steps.component.tsx";

export interface LandingFeature {
  field: FieldKind;
  tag: string;
  title: string;
  body: string;
}

export interface LandingCallout {
  tag: string;
  title: string;
  body: string;
}

export interface LandingStep {
  title: string;
  body: string;
}

export interface LandingCallToAction {
  label: string;
  to: string;
}

/**
 * What the landing page shows. Required here — the controller's contract — and
 * `Partial` on the view, which is composed without props and cloned with them.
 */
export interface LandingViewProps {
  headline: string;
  blurb: string;
  callToAction: LandingCallToAction;
  facts: string[];
  anatomy: LandingCallout[];
  features: LandingFeature[];
  plugins: string[];
  steps: LandingStep[];
}

/**
 * The landing page: a deck rendered in ASCII that follows the cursor, then pulls
 * itself apart as you scroll, then what it does, what it plays with, how to start
 * — and a launch to end on.
 */
export function LandingView(props: Partial<LandingViewProps>) {
  const { headline = "", blurb = "", callToAction = { label: "Open the editor", to: "/config" }, facts = [], anatomy = [], features = [], plugins = [], steps = [] } = props;
  const { scrollYProgress } = useScroll();
  const progress = useSpring(scrollYProgress, { stiffness: 200, damping: 40 });

  return (
    <div className="min-h-screen bg-body text-ink">
      <nav aria-label="Main" className="sticky top-0 z-40 border-b border-line/70 bg-body/75 backdrop-blur-md">
        <motion.div aria-hidden="true" className="absolute top-0 left-0 h-0.5 w-full origin-left bg-signal shadow-[0_0_10px_var(--color-signal)]" style={{ scaleX: progress }} />
        <div className="mx-auto flex max-w-[1440px] flex-wrap items-center justify-between gap-4 px-[clamp(16px,4vw,48px)] py-4">
          <span className="flex items-center gap-3 text-base font-bold tracking-[0.34em]">
            <Mark className="h-6" />
            FLEXDECK
          </span>
          <div className="flex flex-wrap items-center gap-[clamp(14px,3vw,32px)]">
            <a href="#anatomy" className="font-mono text-[11px] tracking-[0.18em] text-muted no-underline hover:text-ink">
              HOW IT WORKS
            </a>
            <a href="#features" className="font-mono text-[11px] tracking-[0.18em] text-muted no-underline hover:text-ink">
              FEATURES
            </a>
            <ThemeToggle />
            <Link
              to={callToAction.to}
              className="inline-flex h-10 items-center rounded-key border border-signal-hi bg-linear-to-b from-signal-hi to-signal px-4 text-[13px] font-semibold text-signal-ink no-underline shadow-key active:translate-y-0.5"
            >
              {callToAction.label}
            </Link>
          </div>
        </div>
      </nav>

      <Hero headline={headline} blurb={blurb} callToAction={callToAction} facts={facts} />
      <Anatomy callouts={anatomy} />
      <FeatureGrid features={features} />
      <Marquee items={plugins} />
      <Steps steps={steps} />
      <Launch callToAction={callToAction} />
    </div>
  );
}
