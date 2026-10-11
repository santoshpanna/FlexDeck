/**
 * flexdeck shared grid layout algorithm — TypeScript reference implementation.
 *
 * ---------------------------------------------------------------------------
 * PARITY IS THE WHOLE POINT OF THIS FILE
 * ---------------------------------------------------------------------------
 * There are three implementations of this algorithm:
 *
 *   packages/grid/src/index.ts          (this file — normative)
 *   crates/flexdeck-grid/src/lib.rs     (Rust — agent/tooling)
 *   android/core/grid/.../GridLayout.kt (Kotlin — tablet renderer)
 *
 * The web configurator renders the preview with the TS one; the tablet renders
 * the real thing with the Kotlin one. If they disagree by even one pixel, the
 * preview lies. They are held together by the golden fixtures in
 * contract/fixtures/grid.
 *
 * ---------------------------------------------------------------------------
 * THE RULES (do not "improve" these without regenerating the fixtures)
 * ---------------------------------------------------------------------------
 * 1. INTEGER ARITHMETIC ONLY. No floats, no Math.round, no accumulating
 *    fractional remainders. Every coordinate is a whole pixel. JavaScript,
 *    Rust and Kotlin all define integer division as truncation toward zero;
 *    that is the only division this file is allowed to do.
 *
 * 2. THE GAP IS ON BOTH OUTER EDGES AND BETWEEN CELLS. So a grid of `n`
 *    columns has `n + 1` gaps. This keeps the layout symmetric and makes the
 *    total width exactly `usableW`.
 *
 * 3. REMAINDER GOES LEFT TO RIGHT. Dividing an inner width that does not
 *    divide evenly leaves `remainder` spare pixels; the first `remainder`
 *    columns each get one extra pixel. Deterministic, and easy to mirror.
 *
 * 4. A SPANNING WIDGET SWALLOWS THE GAPS INSIDE IT. A 2-cell-wide widget is
 *    wider than twice a single cell: it also covers the gap between the two
 *    cells. It still sits inside the outer gaps.
 * ---------------------------------------------------------------------------
 */

/** Geometry inputs for one profile's grid. All values are whole pixels. */
export interface GridSpec {
  cols: number;
  rows: number;
  gapX: number;
  gapY: number;
  usableW: number;
  usableH: number;
}

/** Per-cell geometry along one axis, indexed by cell. */
export interface AxisGeometry {
  /** Absolute offset of each cell's leading edge, in pixels. */
  offsets: number[];
  /** Extent of each cell along the axis (width for columns, height for rows). */
  sizes: number[];
}

export interface GridGeometry {
  /** Columns (horizontal axis). */
  cols: AxisGeometry;
  /** Rows (vertical axis). */
  rows: AxisGeometry;
}

/** A pixel rectangle, relative to the grid's origin. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A widget's cell placement. `w`/`h` are spans in whole cells, both >= 1. */
export interface WidgetSpan {
  id: string;
  col: number;
  row: number;
  w: number;
  h: number;
}

/**
 * Build per-cell offsets and sizes for a single axis.
 *
 * @param count  number of cells along the axis
 * @param usable available pixels along the axis
 * @param gap    gap in pixels, applied before, between and after every cell
 */
export function buildAxis(count: number, usable: number, gap: number): AxisGeometry {
  if (count <= 0) return { offsets: [], sizes: [] };

  // `count` cells have `count + 1` gaps: one at each end, one between each pair.
  const totalGap = gap * (count + 1);
  // Clamp rather than go negative: a grid that cannot fit its own gaps is a
  // configuration error the editor rejects, but the renderer must still
  // produce *something* well-defined instead of NaN or a negative size.
  const inner = Math.max(usable - totalGap, 0);

  const base = Math.trunc(inner / count);
  const remainder = inner - base * count;

  const sizes: number[] = new Array<number>(count);
  for (let i = 0; i < count; i++) {
    sizes[i] = base + (i < remainder ? 1 : 0);
  }

  const offsets: number[] = new Array<number>(count);
  offsets[0] = gap;
  for (let i = 1; i < count; i++) {
    offsets[i] = offsets[i - 1] + sizes[i - 1] + gap;
  }

  return { offsets, sizes };
}

/** Compute both axes for a grid spec. */
export function computeGrid(spec: GridSpec): GridGeometry {
  return {
    cols: buildAxis(spec.cols, spec.usableW, spec.gapX),
    rows: buildAxis(spec.rows, spec.usableH, spec.gapY),
  };
}

/**
 * Pixel rectangle of a widget spanning `w` columns and `h` rows.
 *
 * The returned rectangle covers the widget's cells *and* the gaps between
 * them, so a widget spanning the full grid exactly fills `usableW` x `usableH`.
 */
export function rectOf(geometry: GridGeometry, span: WidgetSpan): Rect {
  const { cols, rows } = geometry;

  const x = cols.offsets[span.col];
  const xEnd = cols.offsets[span.col + span.w - 1] + cols.sizes[span.col + span.w - 1];

  const y = rows.offsets[span.row];
  const yEnd = rows.offsets[span.row + span.h - 1] + rows.sizes[span.row + span.h - 1];

  return { x, y, w: xEnd - x, h: yEnd - y };
}

/**
 * Lay out every widget on a page. This is the function the golden fixtures
 * exercise, and the one both renderers must agree on exactly.
 */
export function layoutPage(spec: GridSpec, widgets: readonly WidgetSpan[]): Record<string, Rect> {
  const geometry = computeGrid(spec);
  const out: Record<string, Rect> = {};
  for (const widget of widgets) {
    out[widget.id] = rectOf(geometry, widget);
  }
  return out;
}

/* ---------------------------------------------------------------------------
 * ORIENTATION
 * ---------------------------------------------------------------------------
 * A deck is authored ONCE, in landscape — the orientation the reference spec is
 * written in — and the panel it is drawn on may be either way up. Rather than
 * authoring a second deck, the DISPLAYED grid is derived from the authored one.
 *
 * The transform is a TRANSPOSE: `col` becomes `row` and vice versa, and a
 * widget's span swaps with it. That is deliberately not a 90-degree rotation,
 * which would move the top-left widget to a different corner; here every widget
 * keeps its ordinal place along each axis, so an 8x4 deck becomes a 4x8 deck with
 * the same arrangement seen the other way up, and transposing twice is the
 * identity. Nothing is lost by rotating.
 *
 * These live in the shared package, and are covered by the same golden fixtures,
 * for the same reason the layout itself does: the preview and the tablet must
 * agree, and a transform implemented twice would eventually disagree.
 * ------------------------------------------------------------------------- */

/** Which way up a deck is being shown. */
export type Orientation = "landscape" | "portrait";

/**
 * The grid as it appears in `orientation`.
 *
 * `usableW`/`usableH` and `gapX`/`gapY` swap along with the axes, because on a
 * rotated panel the long edge is the vertical one: a gap that separated columns
 * now separates rows.
 */
export function orientSpec(spec: GridSpec, orientation: Orientation): GridSpec {
  if (orientation === "landscape") {
    return spec;
  }

  return {
    cols: spec.rows,
    rows: spec.cols,
    gapX: spec.gapY,
    gapY: spec.gapX,
    usableW: spec.usableH,
    usableH: spec.usableW,
  };
}

/** A widget's placement in `orientation`. See `orientSpec`. */
export function orientSpan(span: WidgetSpan, orientation: Orientation): WidgetSpan {
  if (orientation === "landscape") {
    return span;
  }

  return { id: span.id, col: span.row, row: span.col, w: span.h, h: span.w };
}

/**
 * A whole page as it appears in `orientation`.
 *
 * The single entry point a renderer should use. Orienting a spec and its spans
 * separately is easy to do half-way, and a half-oriented page is one whose
 * rectangles can fall outside the grid.
 */
export function orientPage(
  spec: GridSpec,
  widgets: readonly WidgetSpan[],
  orientation: Orientation,
): { spec: GridSpec; widgets: WidgetSpan[] } {
  return {
    spec: orientSpec(spec, orientation),
    widgets: widgets.map((widget) => orientSpan(widget, orientation)),
  };
}

/** A widget as a page stores it: its landscape span, and a portrait one if it has been pinned. */
export interface PlacedWidget {
  span: WidgetSpan;
  /** Where it was put in portrait. Absent: the position is assumed. */
  portrait?: WidgetSpan | null;
}

/** A placement as laid out, saying whether it was chosen or assumed. */
export interface PlacedSpan extends WidgetSpan {
  assumed: boolean;
}

function fitsAt(taken: boolean[][], spec: GridSpec, span: WidgetSpan): boolean {
  if (span.col < 0 || span.row < 0 || span.w < 1 || span.h < 1) return false;
  if (span.col + span.w > spec.cols || span.row + span.h > spec.rows) return false;
  for (let r = span.row; r < span.row + span.h; r++) {
    for (let c = span.col; c < span.col + span.w; c++) if (taken[r]![c]) return false;
  }
  return true;
}

function take(taken: boolean[][], span: WidgetSpan): void {
  for (let r = span.row; r < span.row + span.h; r++) {
    for (let c = span.col; c < span.col + span.w; c++) taken[r]![c] = true;
  }
}

function firstFit(taken: boolean[][], spec: GridSpec, id: string, w: number, h: number): WidgetSpan | null {
  for (let row = 0; row + h <= spec.rows; row++) {
    for (let col = 0; col + w <= spec.cols; col++) {
      const span = { id, col, row, w, h };
      if (fitsAt(taken, spec, span)) return span;
    }
  }
  return null;
}

/**
 * Lay a page out for an orientation, where landscape and portrait each have
 * their own grid and their own positions.
 *
 * Landscape is authored directly. Portrait uses its own grid when there is one
 * (else the landscape grid turned on its side), and each widget's pinned
 * portrait position when it has one. A widget without one gets an ASSUMED
 * position: its landscape span turned on its side if that fits and is free,
 * else the first free cell its size fits in (left to right, top to bottom),
 * else a single cell. Pinned positions are placed first, so an assumed widget
 * never displaces a chosen one. A widget that fits nowhere is left out.
 *
 * With no portrait grid and nothing pinned this is exactly `orientPage` — the
 * old behaviour, unchanged. Mirrored in `GridLayout.placePage` (Kotlin); the
 * two are held together by the same tests.
 */
export function placePage(
  landscape: GridSpec,
  portrait: GridSpec | null | undefined,
  widgets: readonly PlacedWidget[],
  orientation: Orientation,
): { spec: GridSpec; widgets: PlacedSpan[] } {
  if (orientation === "landscape") {
    return { spec: landscape, widgets: widgets.map((widget) => ({ ...widget.span, assumed: false })) };
  }
  const spec = portrait ?? orientSpec(landscape, "portrait");
  const taken = Array.from({ length: Math.max(0, spec.rows) }, () => Array<boolean>(Math.max(0, spec.cols)).fill(false));
  const placed = new Map<string, PlacedSpan>();

  for (const widget of widgets) {
    const pin = widget.portrait;
    if (!pin) continue;
    const span = { ...pin, id: widget.span.id };
    if (fitsAt(taken, spec, span)) {
      take(taken, span);
      placed.set(span.id, { ...span, assumed: false });
    }
  }

  const rest = widgets
    .map((widget, index) => ({ widget, index }))
    .filter(({ widget }) => !placed.has(widget.span.id))
    .sort((a, b) => a.widget.span.row - b.widget.span.row || a.widget.span.col - b.widget.span.col || a.index - b.index);
  for (const { widget } of rest) {
    const turned = orientSpan(widget.span, "portrait");
    const w = Math.min(turned.w, spec.cols);
    const h = Math.min(turned.h, spec.rows);
    const candidate = { ...turned, w, h };
    const span = fitsAt(taken, spec, candidate)
      ? candidate
      : (firstFit(taken, spec, turned.id, w, h) ?? firstFit(taken, spec, turned.id, 1, 1));
    if (!span) continue;
    take(taken, span);
    placed.set(span.id, { ...span, assumed: true });
  }

  return {
    spec,
    widgets: widgets.flatMap((widget) => {
      const span = placed.get(widget.span.id);
      return span ? [span] : [];
    }),
  };
}
