import { useEffect, useState, type ReactNode } from "react";

import { Label, Notice, Segmented, cn } from "@flexdeck/ui";

import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import type { DesktopLocale } from "../../../../services/api/api.ts";
import { DraftInput, control } from "../Flow/fields.tsx";

/** The number styles people write, as decimal and thousands marks. */
const NUMBER_STYLES: readonly { label: string; decimal: string; group: string }[] = [
  { label: "1,234.5", decimal: ".", group: "," },
  { label: "1.234,5", decimal: ",", group: "." },
  { label: "1 234,5", decimal: ",", group: " " },
  { label: "1234.5", decimal: ".", group: "\u0000" },
];

/**
 * How this desktop writes numbers, money, times and temperatures on every
 * key: `{{ x | number }}`, `currency`, `time`, `temp`. Saved as it changes.
 */
export function LocaleCard({ desktopId }: { desktopId: string }) {
  const api = useApi();
  const [locale, setLocale] = useState<DesktopLocale | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api
      .locale(desktopId)
      .then((answer) => live && setLocale(answer))
      .catch((problem: unknown) => live && setError(String(problem instanceof Error ? problem.message : problem)));
    return () => {
      live = false;
    };
  }, [api, desktopId]);

  const save = (change: Partial<DesktopLocale>) => {
    if (!locale) return;
    const next = { ...locale, ...change };
    setLocale(next);
    api
      .saveLocale(desktopId, next)
      .then((answer) => {
        setLocale(answer);
        setError(null);
      })
      .catch((problem: unknown) => setError(String(problem instanceof Error ? problem.message : problem)));
  };

  if (!locale) return error ? <Notice tone="warn">{error}</Notice> : null;
  const style = NUMBER_STYLES.find((s) => s.decimal === locale.decimal && s.group === locale.group)?.label ?? NUMBER_STYLES[0]!.label;

  return (
    <section className="flex flex-col gap-3 rounded-plate border border-line bg-plate p-5">
      <div className="flex flex-col gap-0.5">
        <Label>Formats</Label>
        <span className="text-[12px] text-muted">How every key on this desktop writes numbers, money, times and temperatures.</span>
      </div>
      {error ? <Notice tone="warn">{error}</Notice> : null}
      <Row label="Numbers">
        <Segmented
          size="sm"
          label="Number style"
          value={style}
          onChange={(label) => {
            const chosen = NUMBER_STYLES.find((s) => s.label === label)!;
            save({ decimal: chosen.decimal, group: chosen.group });
          }}
          options={NUMBER_STYLES.map((s) => ({ value: s.label, label: s.label }))}
        />
      </Row>
      <Row label="Money">
        <DraftInput aria-label="Currency" value={locale.currency} maxLength={3} onCommit={(currency) => save({ currency: currency.toUpperCase() })} className={cn(control, "h-8 w-20 text-center font-mono uppercase")} />
        <span className="text-[11px] text-faint">when a key does not name one</span>
      </Row>
      <Row label="Clock">
        <Segmented size="sm" label="Clock" value={locale.hour12 ? "12" : "24"} onChange={(value) => save({ hour12: value === "12" })} options={[{ value: "24", label: "21:41" }, { value: "12", label: "9:41 PM" }]} />
      </Row>
      <Row label="Temperature">
        <Segmented size="sm" label="Temperature" value={locale.fahrenheit ? "f" : "c"} onChange={(value) => save({ fahrenheit: value === "f" })} options={[{ value: "c", label: "°C" }, { value: "f", label: "°F" }]} />
      </Row>
    </section>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center gap-3">
      <span className="w-24 shrink-0 text-[12px] text-muted">{label}</span>
      {children}
    </div>
  );
}
