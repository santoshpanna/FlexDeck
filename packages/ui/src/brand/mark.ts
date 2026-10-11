/**
 * The FlexDeck mark, `[f]`: a key written in text.
 *
 * Defined once, as plain shapes on a 64-unit square, so every place it appears
 * draws the same thing: the `Mark` component, the favicons and app icons that
 * `scripts/gen-brand.mjs` renders, and the Android `MarkView` (which copies these
 * numbers; keep the two in step).
 *
 * Shapes are drawn in the pressed state, the brackets hugging the f. The
 * animation opens them by `BRACKET_TRAVEL` each way and lets them snap back.
 */

/** An axis-aligned rectangle. */
export type MarkRect = { kind: "rect"; x: number; y: number; w: number; h: number };

/**
 * The top-left quarter of a ring centred on (cx, cy): the hook of the f, running
 * from the stem (left) up and over to the terminal (top).
 */
export type MarkArc = { kind: "arc"; cx: number; cy: number; r: number; inner: number };

export type MarkShape = MarkRect | MarkArc;

/** Side of the square every coordinate is measured on. */
export const MARK_BOX = 64;

/** Stroke weight shared by every part, so the glyphs read as one typeface. */
const S = 4.5;

const rect = (x: number, y: number, w: number, h: number): MarkRect => ({ kind: "rect", x, y, w, h });

/** `[`: a bar with serifs top and bottom. */
export const LEFT_BRACKET: readonly MarkShape[] = [rect(15, 12, S, 40), rect(15, 12, 8, S), rect(15, 52 - S, 8, S)];

/** `]`: the left bracket mirrored about the centre. */
export const RIGHT_BRACKET: readonly MarkShape[] = [rect(49 - S, 12, S, 40), rect(41, 12, 8, S), rect(41, 52 - S, 8, S)];

/** `f`: stem, hook, terminal and crossbar. The stem's edges meet the hook's radii exactly. */
export const LETTER_F: readonly MarkShape[] = [
  rect(28.5, 22, S, 30),
  { kind: "arc", cx: 36.5, cy: 22, r: 8, inner: 8 - S },
  rect(36.5, 14, 2.5, S),
  rect(25, 26, 14, S),
];

/** How far each bracket opens, in mark units, when the key is released. */
export const BRACKET_TRAVEL = 4;

/** Corner radius of the app-icon tile, as a fraction of its side. */
export const TILE_RADIUS = 0.22;

/** The tile and glyph colours (dark: the deck never changes with the theme). */
export const MARK_COLORS = {
  tile: "#111113",
  bracket: "#5f5e5a",
  letter: "#ff5a1f",
  ink: "#ecebe6",
} as const;

/** One shape as SVG/VectorDrawable path data, optionally shifted along x. */
export function shapePath(shape: MarkShape, dx = 0): string {
  const n = (v: number) => +v.toFixed(3);
  if (shape.kind === "rect") {
    return `M${n(shape.x + dx)},${n(shape.y)}h${n(shape.w)}v${n(shape.h)}h${n(-shape.w)}z`;
  }
  const { cx, cy, r, inner } = shape;
  const x = cx + dx;
  return `M${n(x - r)},${n(cy)}A${r},${r} 0 0 1 ${n(x)},${n(cy - r)}L${n(x)},${n(cy - inner)}A${inner},${inner} 0 0 0 ${n(x - inner)},${n(cy)}z`;
}

/** Several shapes as one path. */
export function partPath(shapes: readonly MarkShape[], dx = 0): string {
  return shapes.map((shape) => shapePath(shape, dx)).join("");
}

/** Whether the point lies inside the shape. For rasterising without a canvas. */
export function shapeContains(shape: MarkShape, px: number, py: number): boolean {
  if (shape.kind === "rect") {
    return px >= shape.x && px <= shape.x + shape.w && py >= shape.y && py <= shape.y + shape.h;
  }
  if (px > shape.cx || py > shape.cy) return false;
  const d = Math.hypot(px - shape.cx, py - shape.cy);
  return d <= shape.r && d >= shape.inner;
}
