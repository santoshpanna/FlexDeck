import type { Transition, Variants } from "motion/react";

/**
 * The motion vocabulary, in one place so both apps move the same way.
 *
 * Physical things spring: keys, knobs, sheets, the tab pill. Information fades
 * or slides a short way. Nothing waits on an animation to accept input, and
 * every spring retargets from wherever it is when interrupted.
 */
export const spring = {
  /** A key's travel: stiff, settles in ~120ms, a hint of overshoot. */
  key: { type: "spring", stiffness: 700, damping: 30, mass: 0.6 },
  /** Panels, sheets, the selection pill: ~280ms, no visible bounce. */
  panel: { type: "spring", stiffness: 320, damping: 34 },
  /** Large, slow things: a page sliding, a hero settling. */
  soft: { type: "spring", stiffness: 160, damping: 22 },
  /** Toggles: a knob that lands with a small, satisfying overshoot. */
  knob: { type: "spring", stiffness: 520, damping: 26 },
} satisfies Record<string, Transition>;

export const duration = { fast: 0.09, base: 0.18, slow: 0.32 } as const;

export const ease = {
  outExpo: [0.16, 1, 0.3, 1],
  inOut: [0.65, 0, 0.35, 1],
} as const;

/** Delay between siblings entering one after another. */
export const STAGGER = 0.024;

/** A screen or tab body: rises a few pixels and fades in, leaves the same way. */
export const screen: Variants = {
  initial: { opacity: 0, y: 10, scale: 0.995 },
  animate: { opacity: 1, y: 0, scale: 1, transition: { duration: 0.38, ease: ease.outExpo } },
  exit: { opacity: 0, y: -6, transition: { duration: duration.base, ease: ease.inOut } },
};

/** A list whose children enter one after another. */
export const list: Variants = {
  initial: {},
  animate: { transition: { staggerChildren: STAGGER * 2 } },
};

export const item: Variants = {
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0, transition: { duration: 0.42, ease: ease.outExpo } },
  exit: { opacity: 0, height: 0, marginTop: 0, marginBottom: 0, transition: { duration: duration.base } },
};
