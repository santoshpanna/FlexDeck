import type { Background, ImageFit } from "@flexdeck/face";

import { FACE_SWATCHES, drawableColor, gradientCss } from "./face.lib.ts";

/**
 * What fills a surface — a page, a key's face — as one choice: the deck's
 * own, a colour, a fade, a picture, or nothing at all. `Background` keeps
 * these as loose fields; the fill panel edits them as one of these.
 */
export type Fill =
  | { kind: "default" }
  | { kind: "solid"; color: string }
  | { kind: "gradient"; from: string; to: string; angle: number; radial: boolean }
  | { kind: "image"; image: string; fit: ImageFit; dim: number }
  | { kind: "transparent" };

export type FillMode = Fill["kind"];

export const FILL_MODE_LABELS: Record<FillMode, string> = {
  default: "Auto",
  solid: "Colour",
  gradient: "Fade",
  image: "Image",
  transparent: "None",
};

/** The fade a new gradient starts as when nothing came before it. */
const FIRST_GRADIENT = { from: "#9a3412", to: "#1f1f23" };

export const GRADIENT_PRESETS: readonly { name: string; from: string; to: string; angle: number; radial?: boolean }[] = [
  { name: "Ember", from: "#ff5a1f", to: "#7f1d1d", angle: 135 },
  { name: "Dusk", from: "#4c1d95", to: "#1e3a8a", angle: 160 },
  { name: "Lagoon", from: "#115e59", to: "#1e3a8a", angle: 135 },
  { name: "Moss", from: "#3f6212", to: "#1f1f23", angle: 180 },
  { name: "Brass", from: "#facc15", to: "#854d0e", angle: 135 },
  { name: "Steel", from: "#52525b", to: "#18181b", angle: 180 },
  { name: "Glow", from: "#ff5a1f", to: "#1f1f23", angle: 180, radial: true },
  { name: "Night", from: "#1e3a8a", to: "#09090b", angle: 180, radial: true },
];

/** Read a background as one fill. A picture wins over a colour; a fade over a colour. */
export function fillOf(background: Background | undefined): Fill {
  if (!background) return { kind: "default" };
  if (background.transparent) return { kind: "transparent" };
  if (background.image) return { kind: "image", image: background.image, fit: background.fit ?? "IMAGE_FIT_COVER", dim: background.dim ?? 0 };
  const color = drawableColor(background.color);
  const to = drawableColor(background.colorTo);
  if (color && to) return { kind: "gradient", from: color, to, angle: normalAngle(background.angle || 180), radial: background.gradient === "GRADIENT_RADIAL" };
  if (color) return { kind: "solid", color };
  return { kind: "default" };
}

/**
 * Write a fill back into a background, keeping what is not the fill — the
 * graph drawn behind — so changing the fill never drops it. `null` when
 * nothing is left, so the caller can clear the field rather than store `{}`.
 */
export function toBackground(fill: Fill, previous: Background | undefined): Background | null {
  const kept: Background = {};
  if (previous?.graph) kept.graph = previous.graph;
  if (previous?.graphColor) kept.graphColor = previous.graphColor;
  const next: Background = { ...kept };
  switch (fill.kind) {
    case "solid":
      next.color = fill.color;
      break;
    case "gradient": {
      next.color = fill.from;
      next.colorTo = fill.to;
      const angle = normalAngle(fill.angle);
      // 180 is the unset default; 0 is written as 360 so it is not mistaken for unset.
      if (!fill.radial && angle !== 180) next.angle = angle === 0 ? 360 : angle;
      if (fill.radial) next.gradient = "GRADIENT_RADIAL";
      break;
    }
    case "image":
      next.image = fill.image;
      if (fill.fit !== "IMAGE_FIT_COVER") next.fit = fill.fit;
      if (fill.dim > 0) next.dim = fill.dim;
      break;
    case "transparent":
      next.transparent = true;
      break;
    case "default":
      break;
  }
  return Object.keys(next).length > 0 ? next : null;
}

/**
 * Move to another mode carrying over what makes sense: a colour becomes the
 * fade's first stop, a fade's first stop becomes the colour.
 */
export function switchMode(fill: Fill, mode: FillMode): Fill {
  if (fill.kind === mode) return fill;
  const color = fill.kind === "solid" ? fill.color : fill.kind === "gradient" ? fill.from : null;
  switch (mode) {
    case "default":
      return { kind: "default" };
    case "solid":
      return { kind: "solid", color: color ?? FACE_SWATCHES[0]!.color };
    case "gradient":
      return { kind: "gradient", from: color ?? FIRST_GRADIENT.from, to: color && color !== FIRST_GRADIENT.to ? FIRST_GRADIENT.to : "#09090b", angle: 180, radial: false };
    case "image":
      return { kind: "image", image: "", fit: "IMAGE_FIT_COVER", dim: 0 };
    case "transparent":
      return { kind: "transparent" };
  }
}

/** The fill as a CSS `background` for a swatch or preview; `null` for the deck's own or a picture. */
export function fillCss(fill: Fill): string | null {
  if (fill.kind === "solid") return fill.color;
  if (fill.kind === "gradient") return gradientCss(fill.from, fill.to, { angle: fill.angle === 0 ? 360 : fill.angle, gradient: fill.radial ? "GRADIENT_RADIAL" : undefined });
  return null;
}

/** A few words for the trigger: "Rust", "Rust → Graphite 135°", "Picture". */
export function fillLabel(fill: Fill): string {
  switch (fill.kind) {
    case "default":
      return "Default";
    case "solid":
      return colorName(fill.color);
    case "gradient":
      return `${colorName(fill.from)} → ${colorName(fill.to)}${fill.radial ? " · round" : ` ${normalAngle(fill.angle)}°`}`;
    case "image":
      return fill.image ? (fill.image.includes("{{") ? "Picture from a value" : "Picture") : "Picture (none yet)";
    case "transparent":
      return "See-through";
  }
}

/** A swatch's name, or the colour's hex. */
export function colorName(color: string): string {
  const swatch = FACE_SWATCHES.find((s) => s.color === color.toLowerCase());
  return swatch?.name ?? color.toLowerCase();
}

/** Degrees into 0–359. */
export function normalAngle(angle: number): number {
  return ((Math.round(angle) % 360) + 360) % 360;
}

// ---- Colour maths ----------------------------------------------------------

export interface Hsv {
  /** 0–360 */
  h: number;
  /** 0–1 */
  s: number;
  /** 0–1 */
  v: number;
}

export function hexToHsv(hex: string): Hsv {
  const value = drawableColor(hex) ?? "#000000";
  const r = parseInt(value.slice(1, 3), 16) / 255;
  const g = parseInt(value.slice(3, 5), 16) / 255;
  const b = parseInt(value.slice(5, 7), 16) / 255;
  const max = Math.max(r, g, b);
  const delta = max - Math.min(r, g, b);
  let h = 0;
  if (delta > 0) {
    if (max === r) h = 60 * (((g - b) / delta + 6) % 6);
    else if (max === g) h = 60 * ((b - r) / delta + 2);
    else h = 60 * ((r - g) / delta + 4);
  }
  return { h, s: max === 0 ? 0 : delta / max, v: max };
}

export function hsvToHex({ h, s, v }: Hsv): string {
  const c = v * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = v - c;
  const sector = Math.floor((((h % 360) + 360) % 360) / 60);
  const [r, g, b] = [
    [c, x, 0],
    [x, c, 0],
    [0, c, x],
    [0, x, c],
    [x, 0, c],
    [c, 0, x],
  ][sector]!;
  const byte = (n: number) =>
    Math.round((n + m) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${byte(r!)}${byte(g!)}${byte(b!)}`;
}

/** Read what someone typed as a colour: "f60", "#FF6600", "ff6600" → "#ff6600"; else `null`. */
export function parseHex(typed: string): string | null {
  let value = typed.trim().replace(/^#/, "").toLowerCase();
  if (/^[0-9a-f]{3}$/.test(value)) value = value.replace(/./g, (c) => c + c);
  return /^[0-9a-f]{6}$/.test(value) ? `#${value}` : null;
}
