import { computeGrid, type GridSpec, type WidgetSpan } from "@flexdeck/grid";

/**
 * Turning a pointer position into a cell, and a drag into a span.
 *
 * All of it is arithmetic on the SHARED layout algorithm's own per-cell offsets
 * and sizes, never on a re-derived cell size. That is what makes a drag land
 * where the grid actually draws: the gaps and the remainder pixels are already
 * accounted for by `computeGrid`, so snapping cannot disagree with the preview or
 * with the tablet.
 *
 * Pure, because the failures worth catching here are all off-by-one at an edge or
 * a gap, and those are far cheaper to test than to drag.
 */

export interface CellHit {
  col: number;
  row: number;
}

/**
 * Which cell along one axis contains `position`.
 *
 * Clamped to the ends rather than answering "none": a pointer dragged past the
 * last cell should keep asking for the last cell, not stop updating. A point
 * inside a GAP resolves to the cell on its left, which is the one the pointer
 * visually came from.
 */
export function axisIndexAt(offsets: readonly number[], position: number): number {
  if (offsets.length === 0) {
    return 0;
  }

  let index = 0;
  for (let i = 0; i < offsets.length; i += 1) {
    if (offsets[i] <= position) {
      index = i;
    } else {
      break;
    }
  }
  return index;
}

/**
 * The cell a point falls in, in the spec's own pixel space.
 *
 * The caller must divide by the preview scale first — this knows nothing about
 * how large the grid happens to be drawn on screen.
 */
export function cellAt(spec: GridSpec, x: number, y: number): CellHit {
  const geometry = computeGrid(spec);
  return {
    col: axisIndexAt(geometry.cols.offsets, x),
    row: axisIndexAt(geometry.rows.offsets, y),
  };
}

/**
 * The span a resize asks for, given the widget's origin cell and the cell under
 * the pointer.
 *
 * Both axes are clamped to the grid and never below 1: dragging above or to the
 * LEFT of the origin shrinks to a single cell rather than to zero or a negative
 * span, neither of which the grid can express.
 */
export function resizedSpan(
  origin: CellHit,
  spec: GridSpec,
  hit: CellHit,
): { w: number; h: number } {
  return {
    w: clamp(hit.col - origin.col + 1, 1, spec.cols - origin.col),
    h: clamp(hit.row - origin.row + 1, 1, spec.rows - origin.row),
  };
}

/**
 * Which cells have nothing on them, scanning left to right then down.
 *
 * A SPANNING widget occupies every cell it covers, so a 2x2 widget removes four
 * entries — not one. This decides exactly where a new widget may be added, so it
 * is also the set of "+" affordances.
 */
export function emptyCells(spec: GridSpec, spans: readonly WidgetSpan[]): CellHit[] {
  const occupied = new Set<string>();
  for (const span of spans) {
    for (let col = span.col; col < span.col + span.w; col += 1) {
      for (let row = span.row; row < span.row + span.h; row += 1) {
        occupied.add(`${col},${row}`);
      }
    }
  }

  const cells: CellHit[] = [];
  for (let row = 0; row < spec.rows; row += 1) {
    for (let col = 0; col < spec.cols; col += 1) {
      if (!occupied.has(`${col},${row}`)) {
        cells.push({ col, row });
      }
    }
  }
  return cells;
}

function clamp(value: number, min: number, max: number): number {
  // `Math.max(min, max)` first, so a degenerate range (an origin at or past the
  // grid's edge) collapses to `min` instead of returning `max`, which would be
  // smaller than the minimum the grid allows.
  return Math.min(Math.max(value, min), Math.max(min, max));
}
