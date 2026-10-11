import { motion } from "motion/react";

import { Reveal } from "../FeatureGrid/FeatureGrid.component.tsx";

const EXTRAS = ["<system audio/>", "<keyboard/>", "<your plugin/>", "<shell/>", "<midi/>", "<webhooks/>"];

/** What it plays with: plugin names drifting past in two rows, opposite ways. */
export function Marquee({ items }: { items: string[] }) {
  const row = (words: string[], duration: number, reverse: boolean, highlight: (index: number) => string) => (
    <div className="overflow-hidden" aria-hidden="true">
      <motion.div
        className="flex w-max gap-14 font-mono text-[clamp(28px,3.6vw,52px)] tracking-[0.04em] whitespace-nowrap"
        animate={{ x: reverse ? ["-50%", "0%"] : ["0%", "-50%"] }}
        transition={{ duration, ease: "linear", repeat: Infinity }}
      >
        {[...words, ...words].map((word, index) => (
          <span key={index} className={highlight(index % words.length)}>
            {word}
          </span>
        ))}
      </motion.div>
    </div>
  );
  return (
    <section className="flex flex-col gap-6 overflow-hidden border-y border-line/60 py-24">
      <Reveal className="mx-auto w-full max-w-[1440px] px-[clamp(16px,4vw,48px)] pb-6">
        <span className="lbl">§ 04 — Plugins</span>
        <h2 className="m-0 mt-3 text-[clamp(34px,4vw,60px)] leading-none font-semibold tracking-[-0.02em]">Plays well with your stack.</h2>
        <p className="sr-only">Plugins for {items.join(", ")}, and your own.</p>
      </Reveal>
      {row(
        items.map((item) => `[ ${item.toUpperCase()} ]`),
        38,
        false,
        (index) => (index === 1 ? "text-ink" : index === 4 ? "text-signal" : "text-line-strong"),
      )}
      {row(EXTRAS, 46, true, (index) => (index === 2 ? "text-ink" : "text-line-strong"))}
    </section>
  );
}
