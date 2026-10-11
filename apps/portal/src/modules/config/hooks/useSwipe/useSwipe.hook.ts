import { useRef, type PointerEvent as ReactPointerEvent } from "react";

import { swipeDirection, type SwipeDirection } from "../../lib/gesture.lib.ts";

export interface SwipeHandlers {
  onPointerDown: (event: ReactPointerEvent) => void;
  onPointerUp: (event: ReactPointerEvent) => void;
}

/**
 * Horizontal swipe detection for a surface.
 *
 * Distance is measured from pointer-down to pointer-up rather than accumulated
 * across moves: the question is only "did this gesture end to one side of where
 * it began", and a running total would have to invent an answer for a drag that
 * reverses. A gesture that starts on a widget is stopped by that widget's own
 * handler before it reaches here.
 */
export function useSwipe(onSwipe: (direction: SwipeDirection) => void): SwipeHandlers {
  const start = useRef<{ x: number; y: number } | null>(null);

  return {
    onPointerDown: (event) => {
      start.current = { x: event.clientX, y: event.clientY };
    },
    onPointerUp: (event) => {
      const from = start.current;
      start.current = null;
      if (!from) {
        return;
      }
      const direction = swipeDirection(event.clientX - from.x, event.clientY - from.y);
      if (direction) {
        onSwipe(direction);
      }
    },
  };
}
