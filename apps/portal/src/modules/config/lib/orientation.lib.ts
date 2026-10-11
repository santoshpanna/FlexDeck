import { orientSpec, placePage, type GridSpec, type Orientation, type PlacedSpan, type WidgetSpan } from "@flexdeck/grid";

import type { WireConfig, WireGrid, WirePage, WirePlacement, WireProfile } from "../../../services/api/api.ts";
import { gridSpecOf, profileOf } from "./config.lib.ts";
import { withWidget } from "./editing.lib.ts";
import type { GridSizePatch } from "./grid.lib.ts";

/**
 * Landscape and portrait as two layouts.
 *
 * Each orientation has its own grid, and each widget its own place on it. A
 * widget's own col/row/w/h are its LANDSCAPE place. In portrait it sits where
 * someone put it (`portrait`, pinned) or, until then, where `placePage` assumes
 * — so a deck never needs laying out twice to work, but either layout can be
 * made exactly as wanted.
 */

/** Portrait's grid: its own if it has one, else the landscape grid turned on its side. */
export function portraitSpecOf(profile: WireProfile | null): GridSpec {
  const landscape = gridSpecOf(profile);
  const turned = orientSpec(landscape, "portrait");
  const grid = profile?.portraitGrid;
  if (!grid) return turned;
  return {
    cols: grid.cols ?? 0,
    rows: grid.rows ?? 0,
    gapX: grid.gapX ?? 0,
    gapY: grid.gapY ?? 0,
    // The panel's pixels are the device's, not the author's: always the landscape ones, turned.
    usableW: turned.usableW,
    usableH: turned.usableH,
  };
}

/** Whether portrait has a grid of its own, rather than the turned landscape one. */
export function hasOwnPortraitGrid(profile: WireProfile | null): boolean {
  return Boolean(profile?.portraitGrid);
}

function spanOfPlacement(id: string, placement: WirePlacement): WidgetSpan {
  return { id, col: placement.col ?? 0, row: placement.row ?? 0, w: Math.max(1, placement.w ?? 1), h: Math.max(1, placement.h ?? 1) };
}

/** A page laid out for an orientation: its grid, and every widget's span, chosen or assumed. */
export function layoutFor(profile: WireProfile | null, page: WirePage | null, orientation: Orientation): { spec: GridSpec; widgets: PlacedSpan[] } {
  const widgets = (page?.widgets ?? []).map((widget) => ({
    span: { id: widget.id, col: widget.col ?? 0, row: widget.row ?? 0, w: Math.max(1, widget.w ?? 1), h: Math.max(1, widget.h ?? 1) },
    portrait: widget.portrait ? spanOfPlacement(widget.id, widget.portrait) : null,
  }));
  return placePage(gridSpecOf(profile), profile?.portraitGrid ? portraitSpecOf(profile) : null, widgets, orientation);
}

/** proto3 omits zeros, so a placement is written without them — the round trip then changes nothing. */
function toPlacement(span: Omit<WidgetSpan, "id">): WirePlacement {
  const placement: WirePlacement = {};
  if (span.col) placement.col = span.col;
  if (span.row) placement.row = span.row;
  if (span.w) placement.w = span.w;
  if (span.h) placement.h = span.h;
  return placement;
}

function toWireGrid(spec: GridSpec): WireGrid {
  const grid: WireGrid = {};
  if (spec.cols) grid.cols = spec.cols;
  if (spec.rows) grid.rows = spec.rows;
  if (spec.gapX) grid.gapX = spec.gapX;
  if (spec.gapY) grid.gapY = spec.gapY;
  return grid;
}

/**
 * Pin a widget's portrait place. Refused (the same config back) when the span
 * leaves the portrait grid or lands on another widget as portrait shows it now.
 */
export function pinPortrait(config: WireConfig, pageId: string, widgetId: string, span: Omit<WidgetSpan, "id">): WireConfig {
  const profile = profileOf(config);
  const page = profile?.pages?.find((candidate) => candidate.id === pageId) ?? null;
  if (!profile || !page) return config;
  const { spec, widgets } = layoutFor(profile, page, "portrait");
  if (span.col < 0 || span.row < 0 || span.w < 1 || span.h < 1) return config;
  if (span.col + span.w > spec.cols || span.row + span.h > spec.rows) return config;
  const clashes = widgets.some(
    (other) =>
      other.id !== widgetId &&
      other.col < span.col + span.w &&
      span.col < other.col + other.w &&
      other.row < span.row + span.h &&
      span.row < other.row + other.h,
  );
  if (clashes) return config;
  const current = widgets.find((widget) => widget.id === widgetId);
  if (current && !current.assumed && current.col === span.col && current.row === span.row && current.w === span.w && current.h === span.h) {
    return config;
  }
  return withWidget(config, widgetId, (widget) => ({ ...widget, portrait: toPlacement(span) }));
}

/** Let portrait assume the widget's place again. */
export function unpinPortrait(config: WireConfig, widgetId: string): WireConfig {
  return withWidget(config, widgetId, (widget) => {
    if (!widget.portrait) return widget;
    const next = { ...widget };
    delete next.portrait;
    return next;
  });
}

/**
 * Give portrait a grid of its own, starting from the one it shows now. There
 * is no fit check: a pinned widget the new grid cannot hold simply falls back
 * to an assumed place, so a smaller grid never loses a widget's settings.
 */
export function setPortraitGrid(config: WireConfig, patch: GridSizePatch): WireConfig {
  const profile = profileOf(config);
  if (!profile) return config;
  const current = portraitSpecOf(profile);
  const whole = (value: number | undefined, fallback: number, min: number) =>
    Math.max(min, Number.isFinite(value) ? Math.trunc(value as number) : fallback);
  const next: GridSpec = {
    ...current,
    cols: whole(patch.cols, current.cols, 1),
    rows: whole(patch.rows, current.rows, 1),
    gapX: whole(patch.gapX, current.gapX, 0),
    gapY: whole(patch.gapY, current.gapY, 0),
  };
  if (profile.portraitGrid && next.cols === current.cols && next.rows === current.rows && next.gapX === current.gapX && next.gapY === current.gapY) {
    return config;
  }
  return {
    ...config,
    profiles: (config.profiles ?? []).map((candidate) => (candidate.id === profile.id ? { ...candidate, portraitGrid: toWireGrid(next) } : candidate)),
  };
}

/** Go back to the landscape grid turned on its side. Pins are kept; any that no longer fit are assumed. */
export function clearPortraitGrid(config: WireConfig): WireConfig {
  const profile = profileOf(config);
  if (!profile?.portraitGrid) return config;
  return {
    ...config,
    profiles: (config.profiles ?? []).map((candidate) => {
      if (candidate.id !== profile.id) return candidate;
      const next = { ...candidate };
      delete next.portraitGrid;
      return next;
    }),
  };
}
