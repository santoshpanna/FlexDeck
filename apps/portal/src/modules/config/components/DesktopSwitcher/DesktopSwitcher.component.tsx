import { Glyph, Led } from "@flexdeck/ui";

import type { DesktopSummary } from "../../../../services/api/api.ts";

export interface DesktopSwitcherProps {
  /** `null` while the first load is in flight. */
  desktops: DesktopSummary[] | null;
  selected: DesktopSummary | null;
  onSelect: (desktopId: string) => void;
}

/**
 * Which desktop is being configured, with its light. Shown even with one
 * desktop: "which machine am I editing" should never have to be inferred.
 */
export function DesktopSwitcher({ desktops, selected, onSelect }: DesktopSwitcherProps) {
  if (desktops === null) return <span className="font-mono text-[10px] tracking-[0.14em] text-muted">LOADING…</span>;
  if (desktops.length === 0) return <span className="font-mono text-[10px] tracking-[0.14em] text-muted">NO DESKTOP</span>;
  return (
    <label className="relative flex h-9 items-center gap-2 rounded-key border border-[var(--fd-key-line)] bg-linear-to-b from-[var(--fd-key-top)] to-[var(--fd-key-bottom)] pr-8 pl-3 text-[13px] shadow-[inset_0_1px_0_var(--fd-key-glint),0_2px_0_var(--fd-key-base)]">
      <Led state={selected?.online ? "ok" : "off"} />
      <select
        value={selected?.id ?? ""}
        onChange={(event) => onSelect(event.target.value)}
        className="absolute inset-0 cursor-pointer appearance-none opacity-0"
        aria-label="Desktop being configured"
      >
        {desktops.map((desktop) => (
          <option key={desktop.id} value={desktop.id}>
            {desktop.name} — {desktop.online ? "online" : "offline"}
          </option>
        ))}
      </select>
      <span className="pointer-events-none max-w-[160px] truncate">{selected?.name ?? "Choose"}</span>
      <Glyph name="chevronDown" size={12} className="pointer-events-none absolute right-2.5 text-muted" />
    </label>
  );
}
