import { placeBox, placeFace, placeZones, type Face, type Rect } from "@flexdeck/face";

import type { WireWidget } from "../../../services/api/api.ts";

/**
 * The editor's canvas for putting a face's elements anywhere: the key at the
 * tablet's size, and how a dragged box moves.
 */

/** A key on a 1280-wide tablet: one cell, and the gap between cells. */
const CELL = 148;
const GAP = 12;
/** Moves snap to this fraction of the face, unless Alt is held. */
const SNAP = 0.025;
export const NUDGE = 0.01;

/** The key `widget` is, at the tablet's size, and where each of `face`'s elements falls on it. */
export function canvasGeometry(widget: WireWidget, face: Face): { w: number; h: number; faceH: number; rects: Rect[] } {
  const cols = Math.max(1, widget.w ?? 1);
  const rows = Math.max(1, widget.h ?? 1);
  const w = CELL * cols + GAP * (cols - 1);
  const h = CELL * rows + GAP * (rows - 1);
  const faceH = placeZones(widget.face?.zones?.length ?? 0, w, h, 1).face.h;
  const places = placeFace(face.layout, w, faceH, 1);
  const rects = (face.elements ?? []).map((element) => (element.box ? placeBox(element.box, w, faceH) : places[element.slot ?? "SLOT_TITLE"]));
  return { w, h, faceH, rects };
}

const clamp = (n: number, low: number, high: number) => Math.min(high, Math.max(low, n));
const snap = (n: number, free: boolean) => (free ? Math.round(n * 1000) / 1000 : Math.round(n / SNAP) * SNAP);

/** `box` moved by (`dx`, `dy`), or — `resize` — grown by them, kept on the key. */
export function moveBox(box: Rect, dx: number, dy: number, resize: boolean, free: boolean): Rect {
  if (resize) {
    const w = clamp(snap(box.w + dx, free), 0.04, 1 - box.x);
    const h = clamp(snap(box.h + dy, free), 0.04, 1 - box.y);
    return { ...box, w: Math.round(w * 1000) / 1000, h: Math.round(h * 1000) / 1000 };
  }
  const x = clamp(snap(box.x + dx, free), 0, 1 - box.w);
  const y = clamp(snap(box.y + dy, free), 0, 1 - box.h);
  return { ...box, x: Math.round(x * 1000) / 1000, y: Math.round(y * 1000) / 1000 };
}

