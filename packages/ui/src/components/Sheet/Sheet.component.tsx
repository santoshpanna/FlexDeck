import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useEffect, type ReactNode } from "react";

import { cn } from "../../lib/cn.ts";
import { spring } from "../../motion.ts";

export interface SheetProps {
  open: boolean;
  onClose: () => void;
  label: string;
  /** Bottom sheets rise; top sheets drop in, for interruptions like approvals. */
  side?: "bottom" | "top";
  /** Whether a click on the scrim, Escape or a drag closes it. Off for decisions that must be answered. */
  dismissable?: boolean;
  /** `contained` covers the nearest positioned ancestor (a window); `viewport` covers the page and centres the sheet. */
  placement?: "contained" | "viewport";
  children: ReactNode;
  className?: string;
}

/**
 * A sheet over the window: rises with a panel spring, the scrim blurs in, and a
 * bottom sheet can be dragged down to dismiss. Positioned against the nearest
 * positioned ancestor, so it belongs to a window or a panel rather than the
 * whole page.
 */
export function Sheet({
  open,
  onClose,
  label,
  side = "bottom",
  dismissable = true,
  placement = "contained",
  children,
  className,
}: SheetProps) {
  const viewport = placement === "viewport";
  const reduce = useReducedMotion();
  useEffect(() => {
    if (!open || !dismissable) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, dismissable, onClose]);

  const from = side === "bottom" ? "110%" : "-120%";
  return (
    <AnimatePresence>
      {open ? (
        <motion.div
          key="scrim"
          className={cn(viewport ? "fixed" : "absolute", "inset-0 z-40 bg-scrim backdrop-blur-[3px]")}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.22 }}
          onClick={dismissable ? onClose : undefined}
        />
      ) : null}
      {open ? (
        <motion.section
          key="sheet"
          role="dialog"
          aria-modal="true"
          aria-label={label}
          className={cn(
            "z-50 overflow-y-auto rounded-[22px] border border-line bg-plate p-5 shadow-sheet",
            viewport
              ? "fixed inset-x-0 mx-auto w-[min(560px,calc(100%-24px))] max-h-[calc(100vh-48px)]"
              : "absolute inset-x-2.5 max-h-[calc(100%-20px)]",
            side === "bottom" ? (viewport ? "bottom-6" : "bottom-2.5") : viewport ? "top-6 border-signal-line" : "top-2.5 border-signal-line",
            className,
          )}
          initial={reduce ? { opacity: 0 } : { y: from }}
          animate={reduce ? { opacity: 1 } : { y: 0 }}
          exit={reduce ? { opacity: 0 } : { y: from }}
          transition={spring.panel}
          drag={side === "bottom" && dismissable ? "y" : false}
          dragConstraints={{ top: 0, bottom: 0 }}
          dragElastic={{ top: 0, bottom: 0.6 }}
          onDragEnd={(_, info) => {
            if (info.offset.y > 80 || info.velocity.y > 500) onClose();
          }}
        >
          {side === "bottom" && dismissable ? (
            <div aria-hidden="true" className="mx-auto -mt-2 mb-3.5 h-1 w-9 rounded-full bg-line-strong" />
          ) : null}
          {children}
        </motion.section>
      ) : null}
    </AnimatePresence>
  );
}
