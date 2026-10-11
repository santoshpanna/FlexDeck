//! flexdeck shared grid layout algorithm — Rust implementation.
//!
//! The normative spec is `packages/grid/src/index.ts`; read that file for the
//! full rationale. The rules are repeated here because they are easy to break
//! by "tidying up" the arithmetic:
//!
//! 1. **Integer arithmetic only.** No floats, ever. Division truncates toward
//!    zero, which is also what JavaScript and Kotlin do.
//! 2. **The gap borders both ends and sits between cells**, so a grid of `n`
//!    cells has `n + 1` gaps.
//! 3. **Remainder pixels go to the first `remainder` cells**, left to right.
//! 4. **A spanning widget covers the gaps inside it.**
//!
//! This implementation is held to the Kotlin one by the golden fixtures in
//! `contract/fixtures/grid`; `tests/parity.rs` checks it against them.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Geometry inputs for one profile's grid. All values are whole pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridSpec {
    pub cols: i32,
    pub rows: i32,
    pub gap_x: i32,
    pub gap_y: i32,
    pub usable_w: i32,
    pub usable_h: i32,
}

/// Per-cell geometry along one axis, indexed by cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AxisGeometry {
    /// Absolute offset of each cell's leading edge, in pixels.
    pub offsets: Vec<i32>,
    /// Extent of each cell along the axis (width for columns, height for rows).
    pub sizes: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridGeometry {
    /// Columns (horizontal axis).
    pub cols: AxisGeometry,
    /// Rows (vertical axis).
    pub rows: AxisGeometry,
}

/// A pixel rectangle, relative to the grid's origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// A widget's cell placement. `w`/`h` are spans in whole cells, both >= 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetSpan {
    pub id: String,
    pub col: i32,
    pub row: i32,
    pub w: i32,
    pub h: i32,
}

/// Build per-cell offsets and sizes for a single axis.
///
/// * `count` — number of cells along the axis
/// * `usable` — available pixels along the axis
/// * `gap` — gap in pixels, applied before, between and after every cell
pub fn build_axis(count: i32, usable: i32, gap: i32) -> AxisGeometry {
    if count <= 0 {
        return AxisGeometry {
            offsets: Vec::new(),
            sizes: Vec::new(),
        };
    }

    // `count` cells have `count + 1` gaps: one at each end, one between each pair.
    let total_gap = gap * (count + 1);
    // Clamp instead of going negative. A grid that cannot fit its own gaps is a
    // configuration error the editor rejects, but the renderer must still emit
    // something well-defined rather than a negative size.
    let inner = (usable - total_gap).max(0);

    let base = inner / count;
    let remainder = inner - base * count;

    let n = count as usize;

    let mut sizes = vec![0_i32; n];
    for (i, size) in sizes.iter_mut().enumerate() {
        *size = base + i32::from((i as i32) < remainder);
    }

    let mut offsets = vec![0_i32; n];
    offsets[0] = gap;
    for i in 1..n {
        offsets[i] = offsets[i - 1] + sizes[i - 1] + gap;
    }

    AxisGeometry { offsets, sizes }
}

/// Compute both axes for a grid spec.
pub fn compute_grid(spec: &GridSpec) -> GridGeometry {
    GridGeometry {
        cols: build_axis(spec.cols, spec.usable_w, spec.gap_x),
        rows: build_axis(spec.rows, spec.usable_h, spec.gap_y),
    }
}

/// Pixel rectangle of a widget spanning `w` columns and `h` rows.
///
/// The rectangle covers the widget's cells *and* the gaps between them, so a
/// widget spanning the whole grid exactly fills `usable_w` x `usable_h`.
pub fn rect_of(geometry: &GridGeometry, span: &WidgetSpan) -> Rect {
    let first_col = span.col as usize;
    let last_col = (span.col + span.w - 1) as usize;
    let first_row = span.row as usize;
    let last_row = (span.row + span.h - 1) as usize;

    let x = geometry.cols.offsets[first_col];
    let x_end = geometry.cols.offsets[last_col] + geometry.cols.sizes[last_col];

    let y = geometry.rows.offsets[first_row];
    let y_end = geometry.rows.offsets[last_row] + geometry.rows.sizes[last_row];

    Rect {
        x,
        y,
        w: x_end - x,
        h: y_end - y,
    }
}

/// Lay out every widget on a page, keyed by widget id.
pub fn layout_page(spec: &GridSpec, widgets: &[WidgetSpan]) -> BTreeMap<String, Rect> {
    let geometry = compute_grid(spec);
    widgets
        .iter()
        .map(|widget| (widget.id.clone(), rect_of(&geometry, widget)))
        .collect()
}

/* ---------------------------------------------------------------------------
 * ORIENTATION
 * ---------------------------------------------------------------------------
 * A deck is authored ONCE, in landscape — the orientation the reference spec is
 * written in — and the panel it is drawn on may be either way up. The displayed
 * grid is therefore derived from the authored one rather than authored twice.
 *
 * The transform is a TRANSPOSE: `col` becomes `row` and vice versa, and a
 * widget's span swaps with it. Deliberately not a 90-degree rotation, which would
 * move the top-left widget to a different corner; here every widget keeps its
 * ordinal place along each axis, and transposing twice is the identity, so
 * nothing is lost by rotating.
 * ------------------------------------------------------------------------- */

/// Which way up a deck is being shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    /// The authored orientation.
    #[default]
    Landscape,
    Portrait,
}

/// The grid as it appears in `orientation`.
///
/// `usable_w`/`usable_h` and `gap_x`/`gap_y` swap along with the axes, because on
/// a rotated panel the long edge is the vertical one.
pub fn orient_spec(spec: &GridSpec, orientation: Orientation) -> GridSpec {
    match orientation {
        Orientation::Landscape => *spec,
        Orientation::Portrait => GridSpec {
            cols: spec.rows,
            rows: spec.cols,
            gap_x: spec.gap_y,
            gap_y: spec.gap_x,
            usable_w: spec.usable_h,
            usable_h: spec.usable_w,
        },
    }
}

/// A widget's placement in `orientation`. See [`orient_spec`].
pub fn orient_span(span: &WidgetSpan, orientation: Orientation) -> WidgetSpan {
    match orientation {
        Orientation::Landscape => span.clone(),
        Orientation::Portrait => WidgetSpan {
            id: span.id.clone(),
            col: span.row,
            row: span.col,
            w: span.h,
            h: span.w,
        },
    }
}

/// A whole page as it appears in `orientation`.
///
/// The single entry point a renderer should use: orienting a spec and its spans
/// separately is easy to do half-way, and a half-oriented page can place a widget
/// outside the grid.
pub fn orient_page(
    spec: &GridSpec,
    widgets: &[WidgetSpan],
    orientation: Orientation,
) -> (GridSpec, Vec<WidgetSpan>) {
    (
        orient_spec(spec, orientation),
        widgets
            .iter()
            .map(|widget| orient_span(widget, orientation))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(
        cols: i32,
        rows: i32,
        gap_x: i32,
        gap_y: i32,
        usable_w: i32,
        usable_h: i32,
    ) -> GridSpec {
        GridSpec {
            cols,
            rows,
            gap_x,
            gap_y,
            usable_w,
            usable_h,
        }
    }

    fn span(id: &str, col: i32, row: i32, w: i32, h: i32) -> WidgetSpan {
        WidgetSpan {
            id: id.to_string(),
            col,
            row,
            w,
            h,
        }
    }

    #[test]
    fn axis_with_no_gap_splits_evenly() {
        let axis = build_axis(4, 400, 0);
        assert_eq!(axis.sizes, vec![100, 100, 100, 100]);
        assert_eq!(axis.offsets, vec![0, 100, 200, 300]);
    }

    #[test]
    fn gap_borders_both_ends_and_between_cells() {
        let axis = build_axis(3, 100, 10);
        assert_eq!(axis.sizes, vec![20, 20, 20]);
        assert_eq!(axis.offsets, vec![10, 40, 70]);
        // Leading gap + cells + inter-cell gaps + trailing gap == usable exactly.
        assert_eq!(axis.offsets[2] + axis.sizes[2] + 10, 100);
    }

    #[test]
    fn remainder_goes_left_to_right() {
        let axis = build_axis(3, 101, 0);
        assert_eq!(axis.sizes, vec![34, 34, 33]);
        assert_eq!(axis.offsets, vec![0, 34, 68]);
        assert_eq!(axis.offsets[2] + axis.sizes[2], 101);
    }

    #[test]
    fn larger_remainder_stays_ordered_and_never_exceeds_usable() {
        let axis = build_axis(7, 701, 6);
        let consumed = axis.offsets[6] + axis.sizes[6] + 6; // trailing gap
        assert_eq!(consumed, 701);
        for window in axis.sizes.windows(2) {
            assert!(window[0] >= window[1], "sizes must be non-increasing");
        }
    }

    #[test]
    fn degenerate_input_never_produces_negative_sizes() {
        let axis = build_axis(2, 10, 10);
        assert_eq!(axis.sizes, vec![0, 0]);
        assert!(axis.sizes.iter().all(|size| *size >= 0));
    }

    #[test]
    fn zero_cells_is_empty_not_a_crash() {
        let axis = build_axis(0, 400, 8);
        assert!(axis.offsets.is_empty());
        assert!(axis.sizes.is_empty());
    }

    #[test]
    fn full_span_widget_exactly_fills_the_grid() {
        let rect = rect_of(
            &compute_grid(&spec(6, 3, 3, 3, 500, 300)),
            &span("full", 0, 0, 6, 3),
        );
        assert_eq!(
            rect,
            Rect {
                x: 3,
                y: 3,
                w: 494,
                h: 294
            }
        );
    }

    #[test]
    fn spanning_widget_includes_the_gaps_it_covers() {
        let geometry = compute_grid(&spec(3, 1, 10, 10, 100, 50));
        let rect = rect_of(&geometry, &span("pair", 0, 0, 2, 1));
        assert_eq!(
            rect,
            Rect {
                x: 10,
                y: 10,
                w: 50,
                h: 30
            }
        );
    }

    #[test]
    fn layout_page_keys_by_widget_id() {
        let rects = layout_page(
            &spec(4, 4, 0, 0, 400, 400),
            &[span("a", 0, 0, 1, 1), span("c", 3, 3, 1, 1)],
        );
        assert_eq!(
            rects["a"],
            Rect {
                x: 0,
                y: 0,
                w: 100,
                h: 100
            }
        );
        assert_eq!(
            rects["c"],
            Rect {
                x: 300,
                y: 300,
                w: 100,
                h: 100
            }
        );
    }

    #[test]
    fn portrait_swaps_the_axes_the_gaps_and_the_panel_edges() {
        let landscape = spec(8, 4, 3, 7, 1280, 800);

        assert_eq!(orient_spec(&landscape, Orientation::Landscape), landscape);
        assert_eq!(
            orient_spec(&landscape, Orientation::Portrait),
            spec(4, 8, 7, 3, 800, 1280)
        );
    }

    #[test]
    fn transposing_twice_is_the_identity() {
        let landscape = spec(8, 4, 3, 7, 1280, 800);
        let widgets = vec![span("a", 0, 0, 2, 1), span("b", 7, 3, 1, 1)];

        let (once_spec, once_widgets) = orient_page(&landscape, &widgets, Orientation::Portrait);
        let (twice_spec, twice_widgets) =
            orient_page(&once_spec, &once_widgets, Orientation::Portrait);

        assert_eq!(twice_spec, landscape);
        assert_eq!(twice_widgets, widgets);
    }

    #[test]
    fn a_full_width_widget_becomes_a_full_height_one() {
        let landscape = spec(8, 4, 3, 7, 1280, 800);
        let wide = vec![span("bar", 0, 0, 8, 1)];
        let before = layout_page(&landscape, &wide)["bar"];

        let (portrait_spec, portrait_widgets) =
            orient_page(&landscape, &wide, Orientation::Portrait);
        let after = layout_page(&portrait_spec, &portrait_widgets)["bar"];

        // `bar` was as wide as the panel; it is now as tall as the panel is tall,
        // and the inter-cell gaps it swallowed swapped with it.
        assert_eq!(before.w, 1280 - 2 * landscape.gap_x);
        assert_eq!(after.h, before.w);
    }
}
