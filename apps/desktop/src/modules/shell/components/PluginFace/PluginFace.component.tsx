import { cn } from "@flexdeck/ui";

/** A plugin's face: its icon on a keycap, or its first letter when it has none. */
export function PluginFace({ name, icon = null, size = "md" }: { name: string; icon?: string | null; size?: "md" | "lg" }) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "flex shrink-0 items-center justify-center overflow-hidden rounded-[11px] border border-[var(--fd-key-line)] bg-linear-to-b from-[var(--fd-key-top)] to-[var(--fd-key-bottom)] font-mono font-bold text-ink-2 shadow-[inset_0_1px_0_var(--fd-key-glint)]",
        size === "md" ? "size-10 text-[13px]" : "size-14 text-lg",
      )}
    >
      {icon ? <img src={icon} alt="" className="size-full object-cover" /> : name.trim().slice(0, 2).toUpperCase() || "?"}
    </span>
  );
}
