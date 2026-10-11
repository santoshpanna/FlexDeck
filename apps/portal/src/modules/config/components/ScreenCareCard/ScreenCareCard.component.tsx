import { Label, Switch, cn } from "@flexdeck/ui";

import type { ScreenCare } from "../../../../services/api/api.ts";
import { control } from "../Flow/fields.tsx";

const time = (minutes: number | undefined) => {
  const m = minutes ?? 0;
  return `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
};
const minutes = (text: string) => {
  const [h = "0", m = "0"] = text.split(":");
  return Math.min(1439, Math.max(0, Number(h) * 60 + Number(m)));
};

/**
 * Looking after a tablet that shows the deck all day: a slow shift against
 * burn-in, and dimming at night.
 */
export function ScreenCareCard({ care, onChange }: { care: ScreenCare | undefined; onChange: (care: ScreenCare) => void }) {
  const c = care ?? {};
  const set = (change: Partial<ScreenCare>) => onChange({ ...c, ...change });
  return (
    <section className="flex flex-col gap-2.5">
      <Label>The tablet's screen</Label>
      <label className="flex items-start justify-between gap-3 text-[12px] text-muted">
        <span>Shift the deck a little every two minutes, so nothing burns in</span>
        <Switch size="sm" label="Shift against burn-in" checked={c.shift ?? false} onChange={(shift) => set({ shift })} />
      </label>
      <label className="flex items-center justify-between gap-3 text-[12px] text-muted">
        <span>Dim at night</span>
        <Switch size="sm" label="Dim at night" checked={c.dim ?? false} onChange={(dim) => set({ dim, ...(dim && !c.dimFrom && !c.dimTo ? { dimFrom: 22 * 60, dimTo: 7 * 60 } : {}) })} />
      </label>
      {c.dim ? (
        <div className="flex flex-wrap items-center gap-2 text-[12px] text-muted">
          from
          <input aria-label="Dim from" type="time" value={time(c.dimFrom)} onChange={(e) => set({ dimFrom: minutes(e.target.value) })} className={cn(control, "h-8 w-28")} />
          to
          <input aria-label="Dim until" type="time" value={time(c.dimTo)} onChange={(e) => set({ dimTo: minutes(e.target.value) })} className={cn(control, "h-8 w-28")} />
          at
          <input aria-label="Brightness while dimmed" type="range" min={1} max={60} value={c.dimLevel || 10} onChange={(e) => set({ dimLevel: Number(e.target.value) })} className="w-24" />
          <span className="font-mono">{c.dimLevel || 10}%</span>
        </div>
      ) : null}
    </section>
  );
}
