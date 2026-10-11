import { cn } from "@flexdeck/ui";

import { drawableColor } from "../../lib/face.lib.ts";

/**
 * One colour: the deck's default, a row of swatches, and the system picker for
 * anything else. Picking the swatch already chosen clears it.
 */
export function ColorRow({
  label,
  swatches,
  value,
  onChange,
}: {
  label: string;
  swatches: readonly { color: string; name: string }[];
  value: string | undefined;
  onChange: (color: string) => void;
}) {
  const current = drawableColor(value);
  const custom = current !== null && !swatches.some((swatch) => swatch.color === current);
  const ring = "border-signal ring-2 ring-signal/25";
  return (
    <div role="group" aria-label={label} className="flex items-center gap-2">
      <span className="w-10 shrink-0 text-[12px] text-muted">{label}</span>
      <div className="flex flex-wrap items-center gap-1.5">
        <button
          type="button"
          title="Default"
          aria-label={`${label}: default`}
          aria-pressed={current === null}
          onClick={() => onChange("")}
          className={cn("flex size-6 cursor-pointer items-center justify-center rounded-md border bg-linear-to-b from-[var(--fd-key-top)] to-[var(--fd-key-bottom)] font-mono text-[10px] text-faint", current === null ? ring : "border-line hover:border-line-strong")}
        >
          A
        </button>
        {swatches.map((swatch) => {
          const on = swatch.color === current;
          return (
            <button
              key={swatch.color}
              type="button"
              title={swatch.name}
              aria-label={`${label}: ${swatch.name}`}
              aria-pressed={on}
              onClick={() => onChange(on ? "" : swatch.color)}
              className={cn("size-6 cursor-pointer rounded-md border", on ? ring : "border-line hover:border-line-strong")}
              style={{ background: swatch.color }}
            />
          );
        })}
        <label
          title="Any colour"
          className={cn("relative flex size-6 cursor-pointer items-center justify-center overflow-hidden rounded-md border text-[12px]", custom ? ring : "border-dashed border-line-strong text-muted")}
          style={custom && current ? { background: current } : undefined}
        >
          {custom ? null : "+"}
          <input
            type="color"
            aria-label={`${label}: any colour`}
            value={current ?? "#1f1f23"}
            onChange={(event) => onChange(event.target.value)}
            className="absolute inset-0 cursor-pointer opacity-0"
          />
        </label>
      </div>
    </div>
  );
}
