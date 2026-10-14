import type { GridSpec, WidgetSpan } from "@flexdeck/grid";

import { FACES_SCHEMA, GESTURE_STATES_SCHEMA, type WireConfig, type WirePage, type WireProfile, type WireWidget } from "../../../services/api/api.ts";

/**
 * Configuration as it arrives, turned into what the grid algorithm needs.
 *
 * ---------------------------------------------------------------------------
 * EVERY `?? 0` HERE IS LOAD-BEARING
 * ---------------------------------------------------------------------------
 * The agent speaks the proto3 JSON mapping, which **omits default values**. So a
 * gap of zero arrives as an *absent* key, not as `0`. Reading these as "unset,
 * use something sensible instead" would be wrong in the worst way: a user who
 * deliberately sets a gap to 0 would silently get a layout with gaps in it, and
 * the preview would disagree with the tablet.
 *
 * Absent therefore means zero, exactly as it does on the wire, and the defaults
 * are the proto3 ones.
 */
export function gridSpecOf(profile: WireProfile | null): GridSpec {
  const grid = profile?.grid ?? {};

  return {
    cols: grid.cols ?? 0,
    rows: grid.rows ?? 0,
    gapX: grid.gapX ?? 0,
    gapY: grid.gapY ?? 0,
    usableW: grid.usableW ?? 0,
    usableH: grid.usableH ?? 0,
  };
}

export function profileOf(config: WireConfig | null, index = 0): WireProfile | null {
  return config?.profiles?.[index] ?? null;
}

export function pageOf(profile: WireProfile | null, index = 0): WirePage | null {
  return profile?.pages?.[index] ?? null;
}

/**
 * Cell placement for every widget on a page.
 *
 * Spans are clamped up to at least one cell. The grid algorithm has no way to
 * express a zero-sized widget, and passing one through would produce a
 * zero-width rectangle that renders as nothing — a button that silently is not
 * there, which is worse than an obviously wrong one.
 */
export function widgetSpansOf(page: WirePage | null): WidgetSpan[] {
  return (page?.widgets ?? []).map((widget) => ({
    id: widget.id,
    col: widget.col ?? 0,
    row: widget.row ?? 0,
    w: Math.max(1, widget.w ?? 1),
    h: Math.max(1, widget.h ?? 1),
  }));
}

/// What to print on a widget: its label if it has one, otherwise its id.
///
/// Falling back to the id matters — an unlabelled button showing nothing is
/// indistinguishable from a rendering bug, whereas showing `mute` at least says
/// which widget it is.
export function labelOf(widget: WireWidget): string {
  const label = widget.props?.label?.trim();
  return label ? label : widget.id;
}

/// Every page of a profile, in order. Empty when there is no profile.
export function pagesOf(profile: WireProfile | null): WirePage[] {
  return profile?.pages ?? [];
}

/// The page with this id, or `null`.
///
/// Takes `null` so a caller holding "no page chosen yet" does not have to
/// special-case it, and returns `null` for an id the document no longer has —
/// which is the normal state right after that page is deleted.
export function pageById(profile: WireProfile | null, pageId: string | null): WirePage | null {
  if (!pageId) {
    return null;
  }
  return (profile?.pages ?? []).find((page) => page.id === pageId) ?? null;
}

/// Where a page sits in the list, or `-1` when it is not there.
export function pageIndexOf(profile: WireProfile | null, pageId: string | null): number {
  return (profile?.pages ?? []).findIndex((page) => page.id === pageId);
}

/// What to call a page: its name if it has one, otherwise its id.
///
/// The same reasoning as `labelOf`: an unlabelled page showing nothing is
/// indistinguishable from a rendering bug.
export function pageNameOf(page: WirePage): string {
  const name = page.name?.trim();
  return name ? name : page.id;
}

/**
 * The deck raised to the schema what it holds needs: faces need 4, a key's
 * states picked by gestures or by value rules, or a hidden dot, need 5. Never lowered, so an
 * agent too old for something in it refuses the deck by name rather than
 * dropping it unread.
 */
export function withSchemaFor(config: WireConfig): WireConfig {
  const widgets = (config.profiles ?? []).flatMap((profile) => (profile.pages ?? []).flatMap((page) => page.widgets ?? []));
  const needed = widgets.some((widget) => widget.states?.gesture || widget.states?.rules?.length || widget.props?.hideDot || widget.props?.noAutoLight) ? GESTURE_STATES_SCHEMA : FACES_SCHEMA;
  return (config.schema ?? 0) >= needed ? config : { ...config, schema: needed };
}
