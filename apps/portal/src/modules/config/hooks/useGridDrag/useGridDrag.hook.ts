import { useRef, useState, type PointerEvent as ReactPointerEvent, type RefObject } from "react";

import type { GridSpec, WidgetSpan } from "@flexdeck/grid";

import type { WireWidget } from "../../../../services/api/api.ts";
import { cellAt, resizedSpan } from "../../lib/interaction.lib.ts";

export interface GridDragOptions {
  spec: GridSpec;
  /** The rendered frame, so a pointer position can be turned into grid pixels. */
  frameRef: RefObject<HTMLElement | null>;
  /** How many screen pixels one grid pixel occupies — the preview is scaled. */
  scale: number;
  onSelect: (widgetId: string) => void;
  /**
   * A press on a widget that did NOT travel — a click, not a drag.
   *
   * Kept separate from `onSelect` on purpose: `onSelect` fires on pointer-down,
   * so a drag fires it too, and opening a panel there would reflow the deck out
   * from under a drag that is already in progress.
   */
  onTap?: (widgetId: string) => void;
  onMove: (widgetId: string, col: number, row: number) => void;
  onResize: (widgetId: string, w: number, h: number) => void;
}

export interface GridDrag {
  /** The placement a drag is asking for, drawn as a ghost. `null` when idle. */
  candidate: WidgetSpan | null;
  /** Attach to the frame: the drag continues there, not on the widget. */
  frameHandlers: {
    onPointerMove: (event: ReactPointerEvent) => void;
    onPointerUp: (event: ReactPointerEvent) => void;
    onPointerCancel: (event: ReactPointerEvent) => void;
  };
  startMove: (widget: WireWidget, event: ReactPointerEvent) => void;
  startResize: (widget: WireWidget, event: ReactPointerEvent) => void;
}

interface DragState {
  widget: WireWidget;
  mode: "move" | "resize";
  /** Which cell *within* the widget was grabbed, so a move does not jump. */
  grabCol: number;
  grabRow: number;
  candidate: WidgetSpan | null;
}

/**
 * How far the pointer must travel before a press counts as a drag, in screen
 * pixels. Below it the press is a plain click — without this, selecting a widget
 * would nudge it a cell whenever the hand moved a pixel.
 */
const DRAG_THRESHOLD_PX = 4;

/**
 * Dragging a widget to move it, and dragging its grip to resize it.
 *
 * The candidate span is held in a ref as well as in state: state drives the
 * ghost, the ref is what pointer-up reads, so committing can never see a stale
 * render's value.
 *
 * The pointer is CAPTURED BY THE FRAME, not by the widget, and the move/up
 * handlers live on the frame — so a drag that leaves the widget (or the grid)
 * keeps working, and there is only one place that continues it.
 */
export function useGridDrag({
  spec,
  frameRef,
  scale,
  onSelect,
  onTap,
  onMove,
  onResize,
}: GridDragOptions): GridDrag {
  const drag = useRef<DragState | null>(null);
  const origin = useRef<{ x: number; y: number } | null>(null);
  const [candidate, setCandidate] = useState<WidgetSpan | null>(null);

  const hitAt = (event: ReactPointerEvent): { col: number; row: number } | null => {
    const frame = frameRef.current;
    if (!frame || scale <= 0) {
      return null;
    }
    const rect = frame.getBoundingClientRect();
    return cellAt(spec, (event.clientX - rect.left) / scale, (event.clientY - rect.top) / scale);
  };

  const finish = (tapped: boolean) => {
    const state = drag.current;
    drag.current = null;
    origin.current = null;
    setCandidate(null);

    if (!state) {
      return;
    }

    // No candidate means the pointer never travelled; that was a click, and the
    // selection it already made is the whole edit.
    if (!state.candidate) {
      if (tapped) {
        onTap?.(state.widget.id);
      }
      return;
    }

    const w = Math.max(1, state.widget.w ?? 1);
    const h = Math.max(1, state.widget.h ?? 1);

    if (state.mode === "move") {
      const col = state.widget.col ?? 0;
      const row = state.widget.row ?? 0;
      if (state.candidate.col !== col || state.candidate.row !== row) {
        onMove(state.widget.id, state.candidate.col, state.candidate.row);
      }
      return;
    }

    if (state.candidate.w !== w || state.candidate.h !== h) {
      onResize(state.widget.id, state.candidate.w, state.candidate.h);
    }
  };

  const begin = (mode: DragState["mode"], widget: WireWidget, event: ReactPointerEvent) => {
    // Stops the press reaching the swipe surface: dragging a widget must not also
    // turn the page.
    event.stopPropagation();

    if (event.pointerType === "mouse" && event.button !== 0) {
      return;
    }

    // A press selects immediately, so a drag cannot start on a widget the sidebar
    // is not already describing.
    onSelect(widget.id);

    const hit = hitAt(event);
    if (!hit) {
      return;
    }

    frameRef.current?.setPointerCapture(event.pointerId);
    origin.current = { x: event.clientX, y: event.clientY };
    drag.current = {
      widget,
      mode,
      grabCol: hit.col - (widget.col ?? 0),
      grabRow: hit.row - (widget.row ?? 0),
      candidate: null,
    };
  };

  const frameHandlers = {
    onPointerMove: (event: ReactPointerEvent) => {
      const state = drag.current;
      const from = origin.current;
      if (!state || !from) {
        return;
      }

      if (!state.candidate) {
        const travelled = Math.hypot(event.clientX - from.x, event.clientY - from.y);
        if (travelled < DRAG_THRESHOLD_PX) {
          return;
        }
      }

      const hit = hitAt(event);
      if (!hit) {
        return;
      }

      const w = Math.max(1, state.widget.w ?? 1);
      const h = Math.max(1, state.widget.h ?? 1);

      let next: WidgetSpan;
      if (state.mode === "move") {
        next = {
          id: state.widget.id,
          col: clamp(hit.col - state.grabCol, 0, Math.max(0, spec.cols - w)),
          row: clamp(hit.row - state.grabRow, 0, Math.max(0, spec.rows - h)),
          w,
          h,
        };
      } else {
        const cell = { col: state.widget.col ?? 0, row: state.widget.row ?? 0 };
        const span = resizedSpan(cell, spec, hit);
        next = { id: state.widget.id, col: cell.col, row: cell.row, w: span.w, h: span.h };
      }

      state.candidate = next;
      setCandidate(next);
    },
    onPointerUp: () => finish(true),
    onPointerCancel: () => finish(false),
  };

  return {
    candidate,
    frameHandlers,
    startMove: (widget, event) => begin("move", widget, event),
    startResize: (widget, event) => begin("resize", widget, event),
  };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}
