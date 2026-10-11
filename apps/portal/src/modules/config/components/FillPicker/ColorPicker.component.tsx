import { FACE_SWATCHES, drawableColor } from "../../lib/face.lib.ts";
import type { Fill } from "../../lib/fill.lib.ts";
import { FillPicker } from "./FillPicker.component.tsx";

const COLOR_MODES = ["default", "solid"] as const;

/**
 * One colour — text, a state's face, a rule's — in the same panel as a fill,
 * with only the deck's own or a colour to choose. `""` is the deck's own.
 */
export function ColorPicker({
  label,
  value,
  onChange,
  swatches = FACE_SWATCHES,
}: {
  label: string;
  value: string | undefined;
  onChange: (color: string) => void;
  swatches?: readonly { color: string; name: string }[];
}) {
  const color = drawableColor(value);
  const fill: Fill = color ? { kind: "solid", color } : { kind: "default" };
  return <FillPicker label={label} value={fill} modes={COLOR_MODES} swatches={swatches} onChange={(next) => onChange(next.kind === "solid" ? next.color : "")} />;
}
