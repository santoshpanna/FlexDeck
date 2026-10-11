import { motion } from "motion/react";
import type { ReactNode } from "react";

import { ease } from "@flexdeck/ui";
import { AsciiScene, FIELDS } from "@flexdeck/ui/ascii";

import type { LandingFeature } from "../../view/landing.view.tsx";

/**
 * What it does, four ways, each with its own small live figure: a padlock
 * snapping shut on a typed PIN, faders riding, a dry run walking an automation
 * round its loop, a plugin piece locking into place. Cards rise
 * into place as they scroll into view.
 */
export function FeatureGrid({ features }: { features: LandingFeature[] }) {
  return (
    <section id="features" className="mx-auto flex max-w-[1440px] flex-col gap-12 px-[clamp(16px,4vw,48px)] py-32">
      <Reveal className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <span className="lbl">§ 03 — What it does</span>
          <h2 className="m-0 mt-3 max-w-3xl text-[clamp(34px,4vw,60px)] leading-none font-semibold tracking-[-0.02em]">Hardware feel. Software reach.</h2>
        </div>
        <p className="m-0 max-w-sm text-base leading-relaxed text-muted">
          Everything a dedicated controller does, on glass you already own. Every change reaches the tablet as you make it.
        </p>
      </Reveal>
      <div className="grid gap-4 [grid-template-columns:repeat(auto-fit,minmax(min(100%,300px),1fr))]">
        {features.map((feature, index) => (
          <motion.article
            key={feature.tag}
            initial={{ opacity: 0, y: 40, scale: 0.96 }}
            whileInView={{ opacity: 1, y: 0, scale: 1 }}
            viewport={{ once: true, margin: "-80px" }}
            transition={{ duration: 0.7, delay: index * 0.08, ease: ease.outExpo }}
            className="flex flex-col gap-4 overflow-hidden rounded-[22px] border border-line bg-plate p-5"
          >
            <div className="flex justify-center rounded-[14px] border border-line bg-well p-3.5 shadow-well">
              <AsciiScene draw={(t) => FIELDS[feature.field](t, 58, 18)} fps={24} size={8} tone="text-muted" still={3.4} />
            </div>
            <span className="font-mono text-[10px] tracking-[0.2em] text-signal uppercase">{feature.tag}</span>
            <h3 className="-mt-2 mb-0 text-[22px] font-semibold">{feature.title}</h3>
            <p className="-mt-2 mb-0 text-sm leading-relaxed text-muted">{feature.body}</p>
          </motion.article>
        ))}
      </div>
    </section>
  );
}

export function Reveal({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 50, filter: "blur(8px)" }}
      whileInView={{ opacity: 1, y: 0, filter: "blur(0px)" }}
      viewport={{ once: true, margin: "-60px" }}
      transition={{ duration: 0.8, ease: ease.outExpo }}
      className={className}
    >
      {children}
    </motion.div>
  );
}
