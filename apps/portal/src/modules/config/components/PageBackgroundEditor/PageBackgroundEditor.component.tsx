import type { Background } from "@flexdeck/face";

import { Label, Segmented } from "@flexdeck/ui";

import type { VariableChoice, WirePage } from "../../../../services/api/api.ts";
import { FACE_SWATCHES } from "../../lib/face.lib.ts";
import { ColorRow } from "../ColorRow/ColorRow.component.tsx";
import { PicturePicker } from "../PicturePicker/PicturePicker.component.tsx";
import { ValueInput } from "../Flow/fields.tsx";

/**
 * What lies behind every key of a page: a colour, a fade, or one picture
 * across the whole deck. A key made see-through shows it.
 */
export function PageBackgroundEditor({
  page,
  variables,
  onChange,
}: {
  page: WirePage | null;
  variables: readonly VariableChoice[];
  onChange: (background: Background | null) => void;
}) {
  if (!page) return null;
  const background = page.background ?? {};
  const set = (change: Partial<Background>) => onChange({ ...background, ...change });
  return (
    <section className="flex flex-col gap-2.5">
      <div className="flex items-center justify-between">
        <Label>Page background</Label>
        {page.background ? (
          <button type="button" onClick={() => onChange(null)} className="lbl cursor-pointer hover:text-ink">
            Clear
          </button>
        ) : null}
      </div>
      <ColorRow label="Colour" swatches={FACE_SWATCHES} value={background.color} onChange={(color) => set({ color })} />
      {background.color ? <ColorRow label="Fade to" swatches={FACE_SWATCHES} value={background.colorTo} onChange={(colorTo) => set({ colorTo })} /> : null}
      <PicturePicker value={background.image} onPick={(image) => set({ image })} />
      <ValueInput label="Picture" value={background.image ?? ""} variables={variables} kinds={["text"]} placeholder="or follow a value" onCommit={(image) => set({ image: image.trim() })} />
      {background.image ? (
        <div className="flex items-center gap-2">
          <Segmented
            size="sm"
            label="Picture fit"
            value={background.fit ?? "IMAGE_FIT_COVER"}
            onChange={(fit) => set({ fit: fit === "IMAGE_FIT_COVER" ? undefined : fit })}
            options={[
              { value: "IMAGE_FIT_COVER", label: "Fill" },
              { value: "IMAGE_FIT_CONTAIN", label: "Whole" },
            ]}
          />
          <span className="ml-auto text-[12px] text-muted">Darken</span>
          <input aria-label="Darken" type="range" min={0} max={90} step={5} value={background.dim ?? 0} onChange={(event) => set({ dim: Number(event.target.value) })} className="w-24" />
        </div>
      ) : null}
      <p className="m-0 text-[11px] text-faint">Keys stay on top. Make a key's face see-through to let the page show through it.</p>
    </section>
  );
}
