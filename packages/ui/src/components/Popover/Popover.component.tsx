import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";

import { cn } from "../../lib/cn.ts";
import { spring } from "../../motion.ts";

export interface PopoverProps {
  open: boolean;
  onClose: () => void;
  /** What it hangs from; a click on it is not a click outside. */
  anchor: RefObject<HTMLElement | null>;
  label: string;
  /** Its width on a wide screen, in pixels. */
  width?: number;
  children: ReactNode;
  className?: string;
}

/** Narrower than this, a popover rises from the bottom as a sheet. */
const SHEET_BELOW = 640;
const GAP = 8;
const MARGIN = 12;

interface Place {
  sheet: boolean;
  top: number;
  left: number;
  /** Opened upwards, because there was no room below. */
  above: boolean;
}

/**
 * A panel that hangs from the control that opened it: below it, or above when
 * the window has no room, kept inside the window. On a phone it is a bottom
 * sheet instead. Escape or a press outside closes it. Drawn in a portal, so a
 * scrolling or clipped panel does not cut it off.
 */
export function Popover({ open, onClose, anchor, label, width = 320, children, className }: PopoverProps) {
  const panel = useRef<HTMLDivElement>(null);
  const reduce = useReducedMotion();
  const [place, setPlace] = useState<Place | null>(null);

  useLayoutEffect(() => {
    if (!open) return;
    const measure = () => {
      const target = anchor.current;
      if (!target) return;
      if (window.innerWidth < SHEET_BELOW) {
        setPlace({ sheet: true, top: 0, left: 0, above: false });
        return;
      }
      const at = target.getBoundingClientRect();
      const height = panel.current?.offsetHeight ?? 0;
      const roomBelow = window.innerHeight - at.bottom - GAP - MARGIN;
      const above = height > roomBelow && at.top - GAP - MARGIN > roomBelow;
      const top = above ? Math.max(MARGIN, at.top - GAP - height) : Math.min(at.bottom + GAP, Math.max(MARGIN, window.innerHeight - MARGIN - height));
      const left = Math.min(Math.max(MARGIN, at.left), window.innerWidth - MARGIN - width);
      setPlace({ sheet: false, top, left, above });
    };
    measure();
    // Once more after the panel has a height to measure.
    const frame = requestAnimationFrame(measure);
    const observer = new ResizeObserver(measure);
    if (panel.current) observer.observe(panel.current);
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, true);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      window.removeEventListener("resize", measure);
      window.removeEventListener("scroll", measure, true);
    };
  }, [open, anchor, width]);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
      }
    };
    const onDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (panel.current?.contains(target) || anchor.current?.contains(target)) return;
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("pointerdown", onDown, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("pointerdown", onDown, true);
    };
  }, [open, onClose, anchor]);

  useEffect(() => {
    if (!open) setPlace(null);
  }, [open]);

  if (typeof document === "undefined") return null;
  // Known from the first frame, so the entrance is the right one from the start.
  const sheet = place?.sheet ?? window.innerWidth < SHEET_BELOW;
  return createPortal(
    <AnimatePresence>
      {open ? (
        <motion.div
          ref={panel}
          role="dialog"
          aria-label={label}
          className={cn(
            "fixed z-50 overflow-y-auto border border-line bg-plate shadow-sheet",
            sheet ? "inset-x-0 bottom-0 max-h-[85vh] rounded-t-[22px] p-4 pb-6" : "max-h-[calc(100vh-24px)] rounded-[18px] p-3",
            className,
          )}
          style={sheet ? undefined : { top: place?.top ?? -9999, left: place?.left ?? -9999, width }}
          initial={reduce ? { opacity: 0 } : sheet ? { y: "100%" } : { opacity: 0, y: place?.above ? 6 : -6, scale: 0.98 }}
          animate={reduce ? { opacity: 1 } : { opacity: 1, y: 0, scale: 1 }}
          exit={reduce ? { opacity: 0 } : sheet ? { y: "100%" } : { opacity: 0, scale: 0.98 }}
          transition={spring.panel}
        >
          {children}
        </motion.div>
      ) : null}
    </AnimatePresence>,
    document.body,
  );
}
