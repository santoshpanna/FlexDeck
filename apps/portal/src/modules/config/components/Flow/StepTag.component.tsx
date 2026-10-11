import { cn } from "@flexdeck/ui";

import type { BlockKind } from "../../lib/flow.lib.ts";

const TAG: Record<BlockKind, { text: string; tone: string }> = {
  doAction: { text: "DO", tone: "text-ink" },
  if: { text: "IF", tone: "text-warn" },
  wait: { text: "WAIT", tone: "text-[#8b9cff]" },
  repeat: { text: "LOOP", tone: "text-warn" },
  setVariable: { text: "SET", tone: "text-[#b794f4]" },
};

/** A step's kind as a small machined tag; an action shows its plugin's initials. */
export function StepTag({ kind, plugin, className }: { kind: BlockKind; plugin?: string; className?: string }) {
  const tag = TAG[kind];
  const text = kind === "doAction" && plugin ? initials(plugin) : tag.text;
  return (
    <span
      aria-hidden="true"
      className={cn(
        "inline-flex h-7 w-12 shrink-0 items-center justify-center rounded-[7px] border border-line bg-body font-mono text-[10px] font-bold tracking-[0.14em]",
        tag.tone,
        className,
      )}
    >
      {text}
    </span>
  );
}

function initials(name: string): string {
  const words = name.trim().split(/\s+/);
  return (words.length > 1 ? words[0]!.charAt(0) + words[1]!.charAt(0) : name.slice(0, 2)).toUpperCase();
}
