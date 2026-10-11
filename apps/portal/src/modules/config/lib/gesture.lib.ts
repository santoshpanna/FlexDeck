/**
 * Gesture recognition, as arithmetic.
 *
 * A swipe changes the page, so the two cases that matter are the ones that must
 * NOT: a mostly-vertical drag (someone scrolling, or dragging a widget down) and
 * a tiny horizontal jitter. Both are decided here rather than in a component, so
 * the rules are testable without a browser.
 */

export type SwipeDirection = "left" | "right";

/**
 * Which way a gesture went.
 *
 * `dx`/`dy` are the total travel from pointer-down to pointer-up. A gesture is a
 * swipe only when it travelled far enough horizontally AND was more horizontal
 * than vertical; otherwise it is `null`, meaning "not a page change".
 */
export function swipeDirection(dx: number, dy: number, threshold = 60): SwipeDirection | null {
  if (Math.abs(dx) < threshold || Math.abs(dx) <= Math.abs(dy)) {
    return null;
  }
  return dx < 0 ? "left" : "right";
}
