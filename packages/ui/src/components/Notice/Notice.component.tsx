import { AnimatePresence, motion } from "motion/react";
import type { ReactNode } from "react";

import { cn } from "../../lib/cn.ts";
import { spring } from "../../motion.ts";
import { Led } from "../Led/Led.component.tsx";

/**
 * A line that needs reading: an error, a warning, a confirmation. Springs in
 * from the top of its container and collapses away when cleared.
 */
export function Notice({
  tone = "bad",
  children,
  onDismiss,
  className,
}: {
  tone?: "bad" | "warn" | "ok";
  children: ReactNode;
  onDismiss?: () => void;
  className?: string;
}) {
  return (
    <motion.div
      role={tone === "ok" ? "status" : "alert"}
      layout
      initial={{ opacity: 0, y: -8, height: 0 }}
      animate={{ opacity: 1, y: 0, height: "auto" }}
      exit={{ opacity: 0, y: -8, height: 0 }}
      transition={spring.panel}
      className={cn("overflow-hidden", className)}
    >
      <div
        className={cn(
          "flex items-start gap-3 rounded-xl border px-3.5 py-3 text-[13px] leading-snug",
          tone === "bad" && "border-bad/40 bg-bad-soft text-ink",
          tone === "warn" && "border-warn/40 bg-warn-soft text-ink",
          tone === "ok" && "border-ok/40 bg-ok-soft text-ink",
        )}
      >
        <Led state={tone === "ok" ? "ok" : tone === "warn" ? "warn" : "bad"} className="mt-1.5" />
        <div className="min-w-0 flex-1">{children}</div>
        {onDismiss ? (
          <button type="button" onClick={onDismiss} className="cursor-pointer text-muted hover:text-ink" aria-label="Dismiss">
            ×
          </button>
        ) : null}
      </div>
    </motion.div>
  );
}

/** Notices that come and go: wrap the conditional in this so exits animate. */
export function NoticeSlot({ children }: { children: ReactNode }) {
  return <AnimatePresence initial={false}>{children}</AnimatePresence>;
}
