import type { GridSpec } from "@flexdeck/grid";

import type { WireConfig, WireGrid, WirePage } from "../../../services/api/api.ts";
import { gridSpecOf, pagesOf, profileOf } from "./config.lib.ts";
import { spansOf } from "./editing.lib.ts";

/**
 * The grid's own size: how many columns and rows, and the gap between cells.
 *
 * `usableW`/`usableH` are deliberately NOT editable here — they describe the
 * physical panel the deck is drawn on, so they come from the device profile, not
 * from a text field. A person changes the SHAPE of the grid; the panel decides
 * how many pixels that shape is drawn across.
 *
 * Everything is pure, because the interesting rule is a refusal: shrinking the
 * grid must not strand a widget that no longer fits, or a page's layout would be
 * destroyed with no way to tell which widgets went.
 */

/** The four fields a person may change. */
export interface GridSizePatch {
  cols?: number;
  rows?: number;
  gapX?: number;
  gapY?: number;
}

export type GridProblem =
  | { kind: "too_small" }
  | { kind: "orphans_widgets"; count: number };

export function describeGridProblem(problem: GridProblem): string {
  switch (problem.kind) {
    case "too_small":
      return "a grid needs at least one column and one row";
    case "orphans_widgets":
      return problem.count === 1
        ? "one widget would fall off the grid"
        : `${problem.count} widgets would fall off the grid`;
  }
}

/**
 * Whether a grid size is usable, and if not, why.
 *
 * Every page is checked, not only the one on screen: the grid is shared by the
 * whole profile, so a page in the background would otherwise be broken invisibly.
 */
export function gridProblem(spec: GridSpec, pages: readonly WirePage[]): GridProblem | null {
  if (spec.cols < 1 || spec.rows < 1) {
    return { kind: "too_small" };
  }

  let count = 0;
  for (const page of pages) {
    for (const span of spansOf(page)) {
      if (span.col + span.w > spec.cols || span.row + span.h > spec.rows) {
        count += 1;
      }
    }
  }

  return count > 0 ? { kind: "orphans_widgets", count } : null;
}

/**
 * Change the grid's size.
 *
 * Returns the configuration unchanged when the new size is unusable, when a
 * widget would no longer fit, or when nothing actually changed — the same refusal
 * contract as every other edit here.
 */
export function setGridSpec(config: WireConfig, patch: GridSizePatch): WireConfig {
  const profile = profileOf(config);
  if (!profile) {
    return config;
  }

  const current = gridSpecOf(profile);
  const next: GridSpec = {
    cols: truncate(patch.cols ?? current.cols),
    rows: truncate(patch.rows ?? current.rows),
    // A gap cannot be negative; below zero it becomes zero, which the wire then
    // omits (see `toWireGrid`).
    gapX: Math.max(0, truncate(patch.gapX ?? current.gapX)),
    gapY: Math.max(0, truncate(patch.gapY ?? current.gapY)),
    usableW: current.usableW,
    usableH: current.usableH,
  };

  if (gridProblem(next, pagesOf(profile)) !== null) {
    return config;
  }

  if (
    next.cols === current.cols &&
    next.rows === current.rows &&
    next.gapX === current.gapX &&
    next.gapY === current.gapY
  ) {
    return config;
  }

  return {
    ...config,
    profiles: (config.profiles ?? []).map((candidate) =>
      candidate.id === profile.id ? { ...candidate, grid: toWireGrid(next) } : candidate,
    ),
  };
}

function truncate(value: number): number {
  return Number.isFinite(value) ? Math.trunc(value) : 0;
}

/**
 * A grid as the wire wants it: **default values omitted**.
 *
 * proto3 JSON omits defaults, so a gap of zero must be ABSENT rather than `0`.
 * Writing `gapX: 0` would be a difference the round trip silently erases, and
 * every save would look like a change.
 */
function toWireGrid(spec: GridSpec): WireGrid {
  const grid: WireGrid = {};
  if (spec.cols) {
    grid.cols = spec.cols;
  }
  if (spec.rows) {
    grid.rows = spec.rows;
  }
  if (spec.gapX) {
    grid.gapX = spec.gapX;
  }
  if (spec.gapY) {
    grid.gapY = spec.gapY;
  }
  if (spec.usableW) {
    grid.usableW = spec.usableW;
  }
  if (spec.usableH) {
    grid.usableH = spec.usableH;
  }
  return grid;
}
