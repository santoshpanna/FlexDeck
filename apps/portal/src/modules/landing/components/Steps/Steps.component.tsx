import { motion } from "motion/react";

import { ease } from "@flexdeck/ui";

import type { LandingStep } from "../../view/landing.view.tsx";
import { Reveal } from "../FeatureGrid/FeatureGrid.component.tsx";

/** From drawer to deck: three steps, each sliding in behind a big outlined numeral. */
export function Steps({ steps }: { steps: LandingStep[] }) {
  return (
    <section className="mx-auto flex max-w-[1440px] flex-col gap-14 px-[clamp(16px,4vw,48px)] py-36">
      <Reveal>
        <span className="lbl">§ 05 — Three steps</span>
        <h2 className="m-0 mt-3 text-[clamp(34px,4vw,60px)] leading-none font-semibold tracking-[-0.02em]">From drawer to deck.</h2>
      </Reveal>
      <ol className="m-0 grid list-none gap-8 p-0 [grid-template-columns:repeat(auto-fit,minmax(min(100%,280px),1fr))]">
        {steps.map((step, index) => (
          <motion.li
            key={step.title}
            initial={{ opacity: 0, x: -50 }}
            whileInView={{ opacity: 1, x: 0 }}
            viewport={{ once: true, margin: "-60px" }}
            transition={{ duration: 0.8, delay: index * 0.1, ease: ease.outExpo }}
            className="flex flex-col gap-3.5 border-t border-line pt-6"
          >
            <span className="font-mono text-[120px] leading-[0.9] font-medium tracking-[-0.04em] text-transparent [-webkit-text-stroke:1px_var(--color-line-strong)]">
              {String(index + 1).padStart(2, "0")}
            </span>
            <h3 className="m-0 text-[22px] font-semibold">{step.title}</h3>
            <p className="m-0 leading-relaxed text-muted">{step.body}</p>
          </motion.li>
        ))}
      </ol>
    </section>
  );
}
