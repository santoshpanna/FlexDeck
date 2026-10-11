import { cva, type VariantProps } from "class-variance-authority";
import type { HTMLAttributes } from "react";

import { cn } from "../../lib/cn.ts";

export const plateVariants = cva("relative", {
  variants: {
    depth: {
      /** A raised faceplate. */
      raised: "rounded-plate border border-line bg-linear-to-b from-plate-2 to-plate shadow-plate",
      /** A flat panel on the body. */
      flat: "rounded-plate border border-line bg-plate",
      /** A recess: something to look into, like a display or a list. */
      well: "rounded-well border border-line bg-well shadow-well",
    },
    pad: { none: "", sm: "p-3", md: "p-4", lg: "p-5" },
  },
  defaultVariants: { depth: "flat", pad: "md" },
});

export type PlateProps = HTMLAttributes<HTMLDivElement> & VariantProps<typeof plateVariants>;

export function Plate({ depth, pad, className, ...rest }: PlateProps) {
  return <div className={cn(plateVariants({ depth, pad }), className)} {...rest} />;
}

/** The small uppercase mono label over a section. */
export function Label({ className, ...rest }: HTMLAttributes<HTMLSpanElement>) {
  return <span className={cn("lbl", className)} {...rest} />;
}

/** A mono readout: values, codes, counters. */
export function Readout({ className, ...rest }: HTMLAttributes<HTMLSpanElement>) {
  return <span className={cn("font-mono text-[11px] tracking-[0.14em] text-muted uppercase", className)} {...rest} />;
}
