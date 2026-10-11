import { cn } from "../../lib/cn.ts";

/**
 * Print crop marks in the four corners of the nearest positioned ancestor: the
 * technical-poster frame around a figure or a selected key.
 */
export function CropMarks({ inset = 12, className }: { inset?: number; className?: string }) {
  const mark = cn("pointer-events-none absolute size-3.5 border-faint", className);
  return (
    <>
      <span aria-hidden="true" className={cn(mark, "border-t border-l")} style={{ top: inset, left: inset }} />
      <span aria-hidden="true" className={cn(mark, "border-t border-r")} style={{ top: inset, right: inset }} />
      <span aria-hidden="true" className={cn(mark, "border-b border-l")} style={{ bottom: inset, left: inset }} />
      <span aria-hidden="true" className={cn(mark, "border-b border-r")} style={{ bottom: inset, right: inset }} />
    </>
  );
}
