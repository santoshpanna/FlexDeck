package dev.flexdeck.grid

/**
 * flexdeck shared grid layout algorithm — Kotlin implementation.
 *
 * The normative spec is `packages/grid/src/index.ts`; read that file for the
 * full rationale. The rules are repeated here because they are easy to break
 * by "tidying up" the arithmetic:
 *
 *  1. **Integer arithmetic only.** No floats, ever. Kotlin's `/` truncates
 *     toward zero, matching JavaScript and Rust.
 *  2. **The gap borders both ends and sits between cells**, so a grid of `n`
 *     cells has `n + 1` gaps.
 *  3. **Remainder pixels go to the first `remainder` cells**, left to right.
 *  4. **A spanning widget covers the gaps inside it.**
 *
 * `GridLayoutParityTest` checks this against the same golden fixtures the Rust
 * implementation is checked against. The web preview and this renderer must
 * agree pixel for pixel, or the preview lies about what the tablet will show.
 */
data class GridSpec(
  val cols: Int,
  val rows: Int,
  val gapX: Int,
  val gapY: Int,
  val usableW: Int,
  val usableH: Int,
)

/** Per-cell geometry along one axis, indexed by cell. */
data class AxisGeometry(
  /** Absolute offset of each cell's leading edge, in pixels. */
  val offsets: List<Int>,
  /** Extent of each cell along the axis (width for cols, height for rows). */
  val sizes: List<Int>,
)

data class GridGeometry(val cols: AxisGeometry, val rows: AxisGeometry)

/** A pixel rectangle, relative to the grid's origin. */
data class Rect(val x: Int, val y: Int, val w: Int, val h: Int)

/** A widget's cell placement. `w`/`h` are spans in whole cells, both >= 1. */
data class WidgetSpan(val id: String, val col: Int, val row: Int, val w: Int, val h: Int)

/** Which way up a deck is being shown. A deck is AUTHORED in landscape. */
enum class Orientation(val wire: String) {
  LANDSCAPE("landscape"),
  PORTRAIT("portrait"),
  ;

  companion object {
    /**
     * The orientation named by [value], or LANDSCAPE when it is missing or
     * unrecognised — the AUTHORED orientation, because a typo must not silently
     * transpose every deck on the panel.
     */
    fun fromWire(value: String?): Orientation =
      entries.firstOrNull { it.wire == value?.trim()?.lowercase() } ?: LANDSCAPE
  }
}

/** A whole page as it appears in one orientation. */
data class OrientedPage(val spec: GridSpec, val widgets: List<WidgetSpan>)

object GridLayout {
  /**
   * Build per-cell offsets and sizes for a single axis.
   *
   * [count] number of cells along the axis, [usable] available pixels,
   * [gap] gap in pixels applied before, between and after every cell.
   */
  fun buildAxis(count: Int, usable: Int, gap: Int): AxisGeometry {
    if (count <= 0) return AxisGeometry(emptyList(), emptyList())

    // `count` cells have `count + 1` gaps: one at each end, one between each pair.
    val totalGap = gap * (count + 1)
    // Clamp instead of going negative. A grid that cannot fit its own gaps is a
    // configuration error the editor rejects, but the renderer must still emit
    // something well-defined rather than a negative size.
    val inner = (usable - totalGap).coerceAtLeast(0)

    val base = inner / count
    val remainder = inner - base * count

    val sizes = List(count) { i -> base + if (i < remainder) 1 else 0 }

    val offsets = MutableList(count) { 0 }
    offsets[0] = gap
    for (i in 1 until count) {
      offsets[i] = offsets[i - 1] + sizes[i - 1] + gap
    }

    return AxisGeometry(offsets, sizes)
  }

  /** Compute both axes for a grid spec. */
  fun computeGrid(spec: GridSpec): GridGeometry =
    GridGeometry(
      cols = buildAxis(spec.cols, spec.usableW, spec.gapX),
      rows = buildAxis(spec.rows, spec.usableH, spec.gapY),
    )

  /**
   * Pixel rectangle of a widget spanning `w` columns and `h` rows.
   *
   * The rectangle covers the widget's cells *and* the gaps between them, so a
   * widget spanning the whole grid exactly fills `usableW` x `usableH`.
   */
  fun rectOf(geometry: GridGeometry, span: WidgetSpan): Rect {
    val lastCol = span.col + span.w - 1
    val lastRow = span.row + span.h - 1

    val x = geometry.cols.offsets[span.col]
    val xEnd = geometry.cols.offsets[lastCol] + geometry.cols.sizes[lastCol]

    val y = geometry.rows.offsets[span.row]
    val yEnd = geometry.rows.offsets[lastRow] + geometry.rows.sizes[lastRow]

    return Rect(x, y, xEnd - x, yEnd - y)
  }

  /** Lay out every widget on a page, keyed by widget id. */
  fun layoutPage(spec: GridSpec, widgets: List<WidgetSpan>): Map<String, Rect> {
    val geometry = computeGrid(spec)
    return widgets.associate { it.id to rectOf(geometry, it) }
  }

  /**
   * The grid as it appears in [orientation].
   *
   * A deck is authored ONCE, in landscape — the orientation the reference spec is
   * written in. A portrait panel shows the same arrangement with the axes SWAPPED:
   * `col` becomes `row` and vice versa, and a widget's span swaps with it. This is
   * a transpose, not a 90-degree rotation, so every widget keeps its ordinal place
   * along each axis and transposing twice is the identity — nothing is lost by
   * rotating.
   *
   * `usableW`/`usableH` and `gapX`/`gapY` swap too, because on a rotated panel the
   * long edge is the vertical one.
   */
  fun orientSpec(spec: GridSpec, orientation: Orientation): GridSpec =
    when (orientation) {
      Orientation.LANDSCAPE -> spec
      Orientation.PORTRAIT ->
        GridSpec(
          cols = spec.rows,
          rows = spec.cols,
          gapX = spec.gapY,
          gapY = spec.gapX,
          usableW = spec.usableH,
          usableH = spec.usableW,
        )
    }

  /** A widget's placement in [orientation]. See [orientSpec]. */
  fun orientSpan(span: WidgetSpan, orientation: Orientation): WidgetSpan =
    when (orientation) {
      Orientation.LANDSCAPE -> span
      Orientation.PORTRAIT ->
        WidgetSpan(id = span.id, col = span.row, row = span.col, w = span.h, h = span.w)
    }

  /**
   * A whole page as it appears in [orientation].
   *
   * The single entry point a renderer should use: orienting a spec and its spans
   * separately is easy to do half-way, and a half-oriented page can place a widget
   * outside the grid.
   */
  fun orientPage(spec: GridSpec, widgets: List<WidgetSpan>, orientation: Orientation): OrientedPage =
    OrientedPage(
      spec = orientSpec(spec, orientation),
      widgets = widgets.map { orientSpan(it, orientation) },
    )

  /**
   * Lay a page out for [orientation], where landscape and portrait each have
   * their own grid and their own positions. Mirrors `placePage` in
   * packages/grid, case for case.
   *
   * Portrait uses [portrait] when there is one (else the landscape grid turned
   * on its side) and each widget's pinned portrait span when it has one. A
   * widget without one gets an ASSUMED position: its landscape span turned on
   * its side if that fits and is free, else the first free cell its size fits
   * in, else a single cell. Pins are placed first. A widget that fits nowhere
   * is left out. With no portrait grid and no pins, this is [orientPage].
   */
  fun placePage(
    landscape: GridSpec,
    portrait: GridSpec?,
    widgets: List<PlacedWidget>,
    orientation: Orientation,
  ): PlacedPage {
    if (orientation == Orientation.LANDSCAPE) {
      return PlacedPage(landscape, widgets.map { PlacedSpan(it.span, assumed = false) })
    }
    val spec = portrait ?: orientSpec(landscape, Orientation.PORTRAIT)
    val taken = Array(maxOf(0, spec.rows)) { BooleanArray(maxOf(0, spec.cols)) }
    val placed = HashMap<String, PlacedSpan>()

    for (widget in widgets) {
      val pin = widget.portrait ?: continue
      val span = pin.copy(id = widget.span.id)
      if (fitsAt(taken, spec, span)) {
        take(taken, span)
        placed[span.id] = PlacedSpan(span, assumed = false)
      }
    }

    val rest = widgets.withIndex()
      .filter { it.value.span.id !in placed }
      .sortedWith(compareBy({ it.value.span.row }, { it.value.span.col }, { it.index }))
    for ((_, widget) in rest) {
      val turned = orientSpan(widget.span, Orientation.PORTRAIT)
      val candidate = turned.copy(w = minOf(turned.w, spec.cols), h = minOf(turned.h, spec.rows))
      val span = if (fitsAt(taken, spec, candidate)) {
        candidate
      } else {
        firstFit(taken, spec, turned.id, candidate.w, candidate.h) ?: firstFit(taken, spec, turned.id, 1, 1)
      } ?: continue
      take(taken, span)
      placed[span.id] = PlacedSpan(span, assumed = true)
    }

    return PlacedPage(spec, widgets.mapNotNull { placed[it.span.id] })
  }

  private fun fitsAt(taken: Array<BooleanArray>, spec: GridSpec, span: WidgetSpan): Boolean {
    if (span.col < 0 || span.row < 0 || span.w < 1 || span.h < 1) return false
    if (span.col + span.w > spec.cols || span.row + span.h > spec.rows) return false
    for (r in span.row until span.row + span.h) for (c in span.col until span.col + span.w) if (taken[r][c]) return false
    return true
  }

  private fun take(taken: Array<BooleanArray>, span: WidgetSpan) {
    for (r in span.row until span.row + span.h) for (c in span.col until span.col + span.w) taken[r][c] = true
  }

  private fun firstFit(taken: Array<BooleanArray>, spec: GridSpec, id: String, w: Int, h: Int): WidgetSpan? {
    for (row in 0..spec.rows - h) for (col in 0..spec.cols - w) {
      val span = WidgetSpan(id, col, row, w, h)
      if (fitsAt(taken, spec, span)) return span
    }
    return null
  }
}

/** A widget as a page stores it: its landscape span, and its portrait one if pinned. */
data class PlacedWidget(val span: WidgetSpan, val portrait: WidgetSpan? = null)

/** A laid-out span, and whether it was chosen or assumed. */
data class PlacedSpan(val span: WidgetSpan, val assumed: Boolean)

data class PlacedPage(val spec: GridSpec, val widgets: List<PlacedSpan>)
