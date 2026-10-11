import type { WireWidget } from "../../../services/api/api.ts";

/**
 * The swatches offered for a key's face. Dark enough that the deck's light
 * ink stays legible on every one, plus two light ones for dark ink.
 */
export const FACE_SWATCHES: readonly { color: string; name: string }[] = [
  { color: "#1f1f23", name: "Graphite" },
  { color: "#7f1d1d", name: "Oxblood" },
  { color: "#9a3412", name: "Rust" },
  { color: "#854d0e", name: "Ochre" },
  { color: "#3f6212", name: "Moss" },
  { color: "#115e59", name: "Teal" },
  { color: "#1e3a8a", name: "Navy" },
  { color: "#4c1d95", name: "Plum" },
  { color: "#e7e5e4", name: "Bone" },
  { color: "#facc15", name: "Signal yellow" },
];

/** Text colours: the deck's ink, its dark counterpart, and the accent. */
export const INK_SWATCHES: readonly { color: string; name: string }[] = [
  { color: "#f4f4f5", name: "Light" },
  { color: "#111111", name: "Dark" },
  { color: "#ff5a1f", name: "Signal" },
  { color: "#4ade80", name: "Green" },
];

const HEX = /^#[0-9a-f]{6}$/i;

/** `value` when it is a colour the tablet can draw (`#rrggbb`), else `null`. */
export function drawableColor(value: string | undefined): string | null {
  const trimmed = value?.trim() ?? "";
  return HEX.test(trimmed) ? trimmed.toLowerCase() : null;
}

export interface FaceColors {
  /** A CSS background, or `null` for the deck's default key material. */
  background: string | null;
  /** A CSS colour for the label and icon, or `null` for the deck's ink. */
  ink: string | null;
}

/**
 * How the tablet colours this key, so the preview agrees with it.
 *
 * Mirrors `DeckView`: while lit the fixed lit face wins over any colour; a
 * set background is drawn solid; otherwise the default material shows. A
 * colour the tablet would not parse is treated as unset, as there.
 */
export function faceColors(widget: WireWidget, lit: boolean): FaceColors {
  const background = drawableColor(widget.props?.bgColor);
  return {
    background: lit ? null : background,
    ink: drawableColor(widget.props?.fgColor),
  };
}
