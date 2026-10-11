import type { Background } from "@flexdeck/face";

import { Label } from "@flexdeck/ui";

import type { VariableChoice, WirePage } from "../../../../services/api/api.ts";
import { fillOf, toBackground } from "../../lib/fill.lib.ts";
import { FillPicker } from "../FillPicker/FillPicker.component.tsx";

const PAGE_MODES = ["default", "solid", "gradient", "image"] as const;

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
  return (
    <section className="flex flex-col gap-2.5">
      <Label>Page background</Label>
      <FillPicker
        label="Fill"
        value={fillOf(page.background)}
        modes={PAGE_MODES}
        variables={variables}
        onChange={(fill) => onChange(toBackground(fill, page.background))}
        hint="Keys stay on top. Make a key's face see-through to let the page show through it."
      />
    </section>
  );
}
