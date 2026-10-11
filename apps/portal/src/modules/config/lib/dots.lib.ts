/**
 * Which page dots to render, and whether the rail overflows.
 *
 * Dots only read as dots for a handful of pages. Beyond that the rail shows a
 * WINDOW around the current page rather than shrinking every dot into the same
 * smudge — and the caller prints a numeric label beside it, so no page becomes
 * unreachable.
 *
 * Pure, because every interesting failure here is an off-by-one at an end.
 */

/** How many dots the rail shows before it starts windowing. */
export const DOT_WINDOW = 7;

export interface DotWindow {
  /** Page indices to render, ascending and contiguous. */
  indices: number[];
  /** Whether pages are hidden before the window. */
  gapBefore: boolean;
  /** Whether pages are hidden after the window. */
  gapAfter: boolean;
}

export function pageDots(count: number, current: number, maxVisible = DOT_WINDOW): DotWindow {
  const total = Math.max(0, count);
  const all = Array.from({ length: total }, (_, index) => index);

  // A window smaller than three cannot show context on BOTH sides of the current
  // page, so show everything rather than a misleading sliver.
  if (total <= maxVisible || maxVisible < 3) {
    return { indices: all, gapBefore: false, gapAfter: false };
  }

  const clamped = Math.min(Math.max(current, 0), total - 1);
  let start = clamped - Math.floor(maxVisible / 2);
  let end = start + maxVisible - 1;

  if (start < 0) {
    start = 0;
    end = maxVisible - 1;
  }
  if (end > total - 1) {
    end = total - 1;
    start = end - maxVisible + 1;
  }

  return {
    indices: all.slice(start, end + 1),
    gapBefore: start > 0,
    gapAfter: end < total - 1,
  };
}

/**
 * "3 / 12" — the overflow label.
 *
 * One-based, because it is read by a person, and clamped so a stale index can
 * never print a position the deck does not have.
 */
export function pageLabel(current: number, count: number): string {
  const position = Math.min(Math.max(current, 0), Math.max(count - 1, 0)) + 1;
  return `${position} / ${count}`;
}
