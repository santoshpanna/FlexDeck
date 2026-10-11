import { motion, useScroll, useSpring } from "motion/react";

import { SiteNav } from "../../../common/components/SiteNav/SiteNav.component.tsx";
import { useDocumentTitle } from "../../../hooks/useDocumentTitle/useDocumentTitle.hook.ts";
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

const ANCHORS = [
  { label: "How it works", href: "#anatomy" },
  { label: "Features", href: "#features" },
];

/**
 * The landing page: a deck rendered in ASCII that follows the cursor, then pulls
 * itself apart as you scroll, then what it does, what it plays with, how to start
 * — and a launch to end on.
 */
export function LandingView(props: Partial<LandingViewProps>) {
  const { headline = "", blurb = "", callToAction = { label: "Open the editor", to: "/config" }, facts = [], anatomy = [], features = [], plugins = [], steps = [] } = props;
  const { scrollYProgress } = useScroll();
  const progress = useSpring(scrollYProgress, { stiffness: 200, damping: 40 });
  useDocumentTitle();

  return (
    <div className="min-h-screen bg-body text-ink">
      <SiteNav anchors={ANCHORS}>
        <motion.div aria-hidden="true" className="absolute top-0 left-0 h-0.5 w-full origin-left bg-signal shadow-[0_0_10px_var(--color-signal)]" style={{ scaleX: progress }} />
      </SiteNav>

      <Hero headline={headline} blurb={blurb} callToAction={callToAction} facts={facts} />
      <Anatomy callouts={anatomy} />
      <FeatureGrid features={features} />
      <Marquee items={plugins} />
      <Steps steps={steps} />
      <Launch callToAction={callToAction} />
    </div>
  );
}
