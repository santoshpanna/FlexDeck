import { motion, useReducedMotion } from "motion/react";

import { BRACKET_TRAVEL, LEFT_BRACKET, LETTER_F, MARK_BOX, MARK_COLORS, RIGHT_BRACKET, TILE_RADIUS, partPath } from "../../brand/mark.ts";
import { ease } from "../../motion.ts";
import { cn } from "../../lib/cn.ts";

const BRACKETS = { left: partPath(LEFT_BRACKET), right: partPath(RIGHT_BRACKET) };
const LETTER = partPath(LETTER_F);

/** One press cycle: open for a beat, snap shut and light, hold, let go. */
const CYCLE = 2.4;
const TIMES = [0, 0.55, 0.6, 0.94, 1];
const loop = { duration: CYCLE, times: TIMES, ease: ease.outExpo, repeat: Infinity } as const;

export type MarkProps = {
  /** Press the key on a loop. Off, and always under reduced motion, it rests pressed and lit. */
  animated?: boolean;
  /** Draw the dark app-icon tile behind the glyph. Without it the glyph sits on the page. */
  tile?: boolean;
  /** Size it with height (`h-5`); the width follows. */
  className?: string;
  /** Accessible name. Leave out when the word FlexDeck is already next to it. */
  label?: string;
};

/**
 * The FlexDeck mark, `[f]`: a key written in text. Animated, the brackets close
 * in on the f and it lights, like a keypress. Shapes come from `brand/mark.ts`,
 * the same ones the favicons and app icons are rendered from.
 */
export function Mark({ animated = true, tile = false, className, label }: MarkProps) {
  const reduced = useReducedMotion();
  const moving = animated && !reduced;
  const t = BRACKET_TRAVEL;
  // The bare glyph's box leaves room for the brackets to open.
  const viewBox = tile ? `0 0 ${MARK_BOX} ${MARK_BOX}` : `${15 - t - 2} 10 ${34 + 2 * t + 4} 44`;

  return (
    <svg
      viewBox={viewBox}
      className={cn("inline-block h-6 w-auto shrink-0", className)}
      {...(label ? { role: "img", "aria-label": label } : { "aria-hidden": true })}
    >
      {tile ? <rect width={MARK_BOX} height={MARK_BOX} rx={MARK_BOX * TILE_RADIUS} fill={MARK_COLORS.tile} /> : null}
      <g className={tile ? undefined : "fill-faint"} fill={tile ? MARK_COLORS.bracket : undefined}>
        <motion.path d={BRACKETS.left} animate={moving ? { x: [-t, -t, 0, 0, -t] } : { x: 0 }} transition={moving ? loop : { duration: 0 }} />
        <motion.path d={BRACKETS.right} animate={moving ? { x: [t, t, 0, 0, t] } : { x: 0 }} transition={moving ? loop : { duration: 0 }} />
      </g>
      <path d={LETTER} className={tile ? undefined : "fill-ink"} fill={tile ? MARK_COLORS.ink : undefined} />
      <motion.path
        d={LETTER}
        className={tile ? undefined : "fill-signal"}
        fill={tile ? MARK_COLORS.letter : undefined}
        initial={false}
        animate={moving ? { opacity: [0, 0, 1, 1, 0] } : { opacity: 1 }}
        transition={moving ? loop : { duration: 0 }}
      />
    </svg>
  );
}
