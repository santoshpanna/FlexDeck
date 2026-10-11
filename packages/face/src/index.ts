/**
 * Faces: what a key shows, and where — the TypeScript reference.
 *
 * Two things here are computed alike by the editor, the tablet and the agent:
 *
 * - `faceFor` — the face a widget shows in a look. Held to the agent's
 *   `faces.rs` and the tablet's `Faces.kt` by `contract/fixtures/face/derive.json`.
 * - `placeFace` — where each place (slot) of a layout falls on a key of a
 *   given size. Held to the tablet's `FaceLayout.kt` by
 *   `contract/fixtures/face/layout.expected.json`, generated from here with
 *   `pnpm gen:face`.
 *
 * Shapes are the proto3 JSON mapping of `config.proto`: a default value is
 * absent, so an absent slot is the title, an absent kind is text, an absent
 * layout is an icon over a label.
 */

export type FaceLayout =
  | "FACE_LAYOUT_ICON_LABEL"
  | "FACE_LAYOUT_VALUE"
  | "FACE_LAYOUT_ART"
  | "FACE_LAYOUT_SPLIT"
  | "FACE_LAYOUT_DIAL";

export type Slot =
  | "SLOT_TITLE"
  | "SLOT_ICON"
  | "SLOT_VALUE"
  | "SLOT_CAPTION"
  | "SLOT_TOP"
  | "SLOT_BADGE"
  | "SLOT_VISUAL";

export type ElementKind =
  | "ELEMENT_KIND_TEXT"
  | "ELEMENT_KIND_ICON"
  | "ELEMENT_KIND_BAR"
  | "ELEMENT_KIND_GAUGE"
  | "ELEMENT_KIND_GRAPH"
  | "ELEMENT_KIND_CLOCK"
  | "ELEMENT_KIND_TIMER";

export type TextSize =
  | "TEXT_SIZE_AUTO"
  | "TEXT_SIZE_XS"
  | "TEXT_SIZE_S"
  | "TEXT_SIZE_M"
  | "TEXT_SIZE_L"
  | "TEXT_SIZE_XL"
  | "TEXT_SIZE_FIT";

export type TextAlign = "TEXT_ALIGN_AUTO" | "TEXT_ALIGN_START" | "TEXT_ALIGN_CENTER" | "TEXT_ALIGN_END";
export type Font = "FONT_SANS" | "FONT_MONO" | "FONT_SERIF";
export type GraphStyle = "GRAPH_STYLE_AREA" | "GRAPH_STYLE_LINE" | "GRAPH_STYLE_BARS";
export type ImageFit = "IMAGE_FIT_COVER" | "IMAGE_FIT_CONTAIN";

export interface Rule {
  when?: string;
  color?: string;
  pulse?: boolean;
}

export interface TextStyle {
  size?: TextSize;
  bold?: boolean;
  align?: TextAlign;
  font?: Font;
  maxLines?: number;
  /** Too long for its place: one line scrolling sideways. */
  marquee?: boolean;
}

export interface FaceElement {
  slot?: Slot;
  kind?: ElementKind;
  template?: string;
  variable?: string;
  min?: number;
  max?: number;
  text?: TextStyle;
  color?: string;
  rules?: Rule[];
  clock?: { analog?: boolean; seconds?: boolean; format?: string; zone?: string };
  graph?: { style?: GraphStyle; points?: number };
  /** Put anywhere: fractions of the face, 0–1. Absent: its slot's place. */
  box?: Rect;
}

export interface Background {
  color?: string;
  colorTo?: string;
  image?: string;
  fit?: ImageFit;
  dim?: number;
  graph?: string;
  graphColor?: string;
  transparent?: boolean;
}

export interface Face {
  layout?: FaceLayout;
  background?: Background;
  elements?: FaceElement[];
  rules?: Rule[];
  /** Buttons along the bottom strip, each with flows of its own. */
  zones?: Zone[];
}

/** One button of a face's bottom strip: a music key's previous, play or next. */
export interface Zone {
  name: string;
  icon?: string;
  /** While this holds, `iconOn` shows instead. */
  when?: string;
  iconOn?: string;
}

export interface FaceLook {
  id: string;
  label?: string;
  bgColor?: string;
  fgColor?: string;
  icon?: string;
  face?: Face;
}

/** The parts of a widget a face is worked out from. */
export interface FaceSource {
  id: string;
  props?: {
    label?: string;
    icon?: string;
    bgColor?: string;
    fgColor?: string;
    labelTemplate?: string;
    iconTemplate?: string;
  };
  face?: Face;
  states?: { looks?: FaceLook[] };
}

export const LAYOUTS: readonly FaceLayout[] = [
  "FACE_LAYOUT_ICON_LABEL",
  "FACE_LAYOUT_VALUE",
  "FACE_LAYOUT_ART",
  "FACE_LAYOUT_SPLIT",
  "FACE_LAYOUT_DIAL",
];

export const SLOTS: readonly Slot[] = [
  "SLOT_TITLE",
  "SLOT_ICON",
  "SLOT_VALUE",
  "SLOT_CAPTION",
  "SLOT_TOP",
  "SLOT_BADGE",
  "SLOT_VISUAL",
];

const set = (value: string | undefined): string => (value ?? "").trim();

function firstSet(...candidates: (string | undefined)[]): string {
  for (const candidate of candidates) {
    if (set(candidate)) {
      return set(candidate);
    }
  }
  return "";
}

/** Drop absent and default fields, as the proto3 JSON mapping does. */
function compact<T extends object>(value: T): T {
  const out: Record<string, unknown> = {};
  for (const [key, field] of Object.entries(value)) {
    if (field === undefined || field === "" || field === null) continue;
    if (Array.isArray(field) && field.length === 0) continue;
    out[key] = field;
  }
  return out as T;
}

/**
 * The face schema 3 drew from `props`: an icon over a label in the props'
 * colours. A live label wins over the plain one, a live icon over the plain
 * icon.
 */
export function derive(props: FaceSource["props"]): Face {
  const p = props ?? {};
  const elements: FaceElement[] = [];
  const icon = firstSet(p.iconTemplate, p.icon);
  if (icon) {
    elements.push(compact({ slot: "SLOT_ICON" as Slot, kind: "ELEMENT_KIND_ICON" as ElementKind, template: icon, color: p.fgColor }));
  }
  elements.push(compact({ template: firstSet(p.labelTemplate, p.label), color: p.fgColor }));
  const background = set(p.bgColor);
  return compact({ background: background ? { color: background } : undefined, elements });
}

/** The face `widget` shows in look `lookId` (absent or unknown: its own). */
export function faceFor(widget: FaceSource, lookId?: string): Face {
  const look = lookId ? widget.states?.looks?.find((candidate) => candidate.id === lookId) : undefined;
  if (!look) {
    return widget.face ?? derive(widget.props);
  }
  if (look.face) {
    return look.face;
  }
  if (!widget.face) {
    const props = { ...(widget.props ?? {}) };
    if (set(look.label)) props.label = look.label;
    if (set(look.icon)) props.icon = look.icon;
    if (set(look.bgColor)) props.bgColor = look.bgColor;
    if (set(look.fgColor)) props.fgColor = look.fgColor;
    return derive(props);
  }
  const face: Face = structuredClone(widget.face);
  const elements = face.elements ?? [];
  if (set(look.label)) {
    const title = elements.find((e) => (e.slot ?? "SLOT_TITLE") === "SLOT_TITLE" && (e.kind ?? "ELEMENT_KIND_TEXT") === "ELEMENT_KIND_TEXT");
    if (title) title.template = look.label;
  }
  if (set(look.icon)) {
    const icon = elements.find((e) => e.kind === "ELEMENT_KIND_ICON");
    if (icon) icon.template = look.icon;
  }
  if (set(look.bgColor)) {
    face.background = { ...(face.background ?? {}), color: look.bgColor };
  }
  if (set(look.fgColor)) {
    for (const element of elements) {
      const kind = element.kind ?? "ELEMENT_KIND_TEXT";
      if (kind === "ELEMENT_KIND_TEXT" || kind === "ELEMENT_KIND_ICON") element.color = look.fgColor;
    }
  }
  return face;
}

/**
 * A countdown as a key shows it: `4:07`, `1:02:00`. Whole seconds, rounded
 * up, so it reads 0:00 only when the time is really up.
 */
export function countdownText(seconds: number | undefined): string {
  if (seconds === undefined || !Number.isFinite(seconds)) return "–:––";
  const total = Math.max(0, Math.ceil(seconds - 1e-9));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** Seconds left now, from what the agent sent (`left`, at `sentAt` ms) and whether it runs. */
export function timerLeft(left: number | undefined, running: boolean, sentAt: number, now: number): number | undefined {
  if (left === undefined) return undefined;
  return running ? Math.max(0, left - (now - sentAt) / 1000) : left;
}

/** Whether a template has holes, so its value comes from the agent. */
export function isLive(template: string | undefined): boolean {
  return (template ?? "").includes("{{");
}

// ---- Places ----------------------------------------------------------------

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Text sizes at unit 1, in pixels. The tablet's unit is its long edge / 1280. */
export const TEXT_PX: Record<Exclude<TextSize, "TEXT_SIZE_AUTO" | "TEXT_SIZE_FIT">, number> = {
  TEXT_SIZE_XS: 10,
  TEXT_SIZE_S: 12,
  TEXT_SIZE_M: 15,
  TEXT_SIZE_L: 22,
  TEXT_SIZE_XL: 32,
};

/** A line of text `px` tall needs this much room. */
const band = (px: number): number => px * 1.3;

/** Two decimals: what the fixtures hold, so float noise never fails a parity test. */
const r2 = (n: number): number => Math.round(n * 100) / 100;

function rect(x: number, y: number, w: number, h: number): Rect {
  return { x: r2(x), y: r2(y), w: r2(Math.max(0, w)), h: r2(Math.max(0, h)) };
}

/**
 * A strip of `count` equal zones along the bottom of a key `w` × `h`, and the
 * room left above it, where the rest of the face is placed. No zones: the
 * face has the whole key.
 */
export function placeZones(count: number, w: number, h: number, unit: number): { face: Rect; zones: Rect[] } {
  if (count <= 0) return { face: rect(0, 0, w, h), zones: [] };
  const strip = Math.min(h * 0.5, Math.max(30 * unit, h * 0.3));
  const top = h - strip;
  const zones = Array.from({ length: count }, (_, i) => rect((w * i) / count, top, w / count, strip));
  return { face: rect(0, 0, w, top), zones };
}

/** Where a freely placed element falls on a face `w` × `h`: its box's fractions, in pixels. */
export function placeBox(box: Rect, w: number, h: number): Rect {
  return rect(box.x * w, box.y * h, box.w * w, box.h * h);
}

/** A place as fractions of a face `w` × `h`, rounded to thousandths: what a box holds. */
export function boxOf(place: Rect, w: number, h: number): Rect {
  const f = (n: number) => Math.round(n * 1000) / 1000;
  const x = Math.min(1, Math.max(0, f(place.x / w)));
  const y = Math.min(1, Math.max(0, f(place.y / h)));
  return { x, y, w: Math.max(0.02, Math.min(f(1 - x), f(place.w / w))), h: Math.max(0.02, Math.min(f(1 - y), f(place.h / h))) };
}

/** Which zone a touch at (`x`, `y`) on the key lands on; -1 for none. */
export function zoneAt(count: number, w: number, h: number, unit: number, x: number, y: number): number {
  const { zones } = placeZones(count, w, h, unit);
  return zones.findIndex((z) => x >= z.x && x < z.x + z.w && y >= z.y && y <= z.y + z.h);
}

function square(area: Rect, scale: number): Rect {
  const side = Math.min(area.w, area.h) * scale;
  return rect(area.x + (area.w - side) / 2, area.y + (area.h - side) / 2, side, side);
}

/**
 * Where each place of `layout` falls on a key `w` × `h` pixels, at the
 * tablet's `unit`. Every layout has every place; one a layout does not use
 * sits where it reads least badly.
 */
export function placeFace(layout: FaceLayout | undefined, w: number, h: number, unit: number): Record<Slot, Rect> {
  const pad = Math.min(w, h) * 0.08;
  const X = pad;
  const Y = pad;
  const W = w - pad * 2;
  const H = h - pad * 2;
  const m = Math.min(W, H);
  const gap = m * 0.04;
  const xs = band(TEXT_PX.TEXT_SIZE_XS * unit);
  const s = band(TEXT_PX.TEXT_SIZE_S * unit);
  const md = band(TEXT_PX.TEXT_SIZE_M * unit);
  const badgeH = xs * 1.1;
  const badge = rect(X + W - badgeH * 2.2, Y, badgeH * 2.2, badgeH);
  const top = rect(X, Y, W, xs);
  const whole = rect(X, Y, W, H);

  switch (layout ?? "FACE_LAYOUT_ICON_LABEL") {
    case "FACE_LAYOUT_VALUE": {
      const head = rect(X, Y, W, s);
      const caption = rect(X, Y + H - s, W, s);
      const value = rect(X, head.y + head.h, W, caption.y - (head.y + head.h));
      return {
        SLOT_TITLE: head,
        SLOT_TOP: head,
        SLOT_ICON: rect(X, Y, s, s),
        SLOT_VALUE: value,
        SLOT_CAPTION: caption,
        SLOT_BADGE: badge,
        SLOT_VISUAL: whole,
      };
    }
    case "FACE_LAYOUT_ART": {
      const caption = rect(X, Y + H - s, W, s);
      const title = rect(X, caption.y - md, W, md);
      const strip = Math.max(4 * unit, m * 0.05);
      const visual = rect(X, title.y - gap - strip, W, strip);
      const value = rect(X, Y, W, visual.y - gap - Y);
      return {
        SLOT_TITLE: title,
        SLOT_TOP: top,
        SLOT_ICON: square(value, 0.6),
        SLOT_VALUE: value,
        SLOT_CAPTION: caption,
        SLOT_BADGE: badge,
        SLOT_VISUAL: visual,
      };
    }
    case "FACE_LAYOUT_SPLIT": {
      if (W >= H) {
        const side = Math.min(H, W * 0.45);
        const visual = rect(X, Y + (H - side) / 2, side, side);
        const tx = X + side + gap;
        const tw = W - side - gap;
        const middle = Y + H / 2;
        const title = rect(tx, middle - md, tw, md);
        return {
          SLOT_TITLE: title,
          SLOT_VALUE: title,
          SLOT_TOP: rect(tx, Y, tw, xs),
          SLOT_ICON: square(visual, 0.8),
          SLOT_CAPTION: rect(tx, middle, tw, s),
          SLOT_BADGE: badge,
          SLOT_VISUAL: visual,
        };
      }
      const side = Math.min(W, H * 0.55);
      const visual = rect(X + (W - side) / 2, Y, side, side);
      const title = rect(X, Y + side + gap, W, md);
      return {
        SLOT_TITLE: title,
        SLOT_VALUE: title,
        SLOT_TOP: top,
        SLOT_ICON: square(visual, 0.8),
        SLOT_CAPTION: rect(X, title.y + title.h, W, s),
        SLOT_BADGE: badge,
        SLOT_VISUAL: visual,
      };
    }
    case "FACE_LAYOUT_DIAL": {
      const caption = rect(X, Y + H - s, W, s);
      const room = caption.y - gap - Y;
      const side = Math.min(W, room);
      const visual = rect(X + (W - side) / 2, Y + (room - side) / 2, side, side);
      const value = square(visual, 0.6);
      return {
        SLOT_TITLE: caption,
        SLOT_TOP: top,
        SLOT_ICON: value,
        SLOT_VALUE: value,
        SLOT_CAPTION: caption,
        SLOT_BADGE: badge,
        SLOT_VISUAL: visual,
      };
    }
    default: {
      const title = rect(X, Y + H - md, W, md);
      const area = rect(X, Y, W, title.y - gap - Y);
      return {
        SLOT_TITLE: title,
        SLOT_CAPTION: title,
        SLOT_TOP: top,
        SLOT_ICON: square(area, 0.7),
        SLOT_VALUE: area,
        SLOT_BADGE: badge,
        SLOT_VISUAL: area,
      };
    }
  }
}

/** How text in a place is drawn when its element does not say. */
export interface PlaceStyle {
  size: Exclude<TextSize, "TEXT_SIZE_AUTO">;
  align: Exclude<TextAlign, "TEXT_ALIGN_AUTO">;
  bold: boolean;
  maxLines: number;
  /** Drawn at reduced strength: a caption, a top line. */
  quiet: boolean;
}

export function placeStyle(layout: FaceLayout | undefined, slot: Slot): PlaceStyle {
  const leading = layout === "FACE_LAYOUT_ART" || layout === "FACE_LAYOUT_SPLIT";
  const align = leading ? "TEXT_ALIGN_START" : "TEXT_ALIGN_CENTER";
  switch (slot) {
    case "SLOT_VALUE":
      return { size: "TEXT_SIZE_FIT", align: leading ? "TEXT_ALIGN_START" : "TEXT_ALIGN_CENTER", bold: true, maxLines: 1, quiet: false };
    case "SLOT_CAPTION":
      return { size: "TEXT_SIZE_S", align, bold: false, maxLines: 1, quiet: true };
    case "SLOT_TOP":
      return { size: "TEXT_SIZE_XS", align, bold: false, maxLines: 1, quiet: true };
    case "SLOT_BADGE":
      return { size: "TEXT_SIZE_XS", align: "TEXT_ALIGN_CENTER", bold: true, maxLines: 1, quiet: false };
    default: {
      // The VALUE layout's title is its small top line.
      if (layout === "FACE_LAYOUT_VALUE") {
        return { size: "TEXT_SIZE_S", align, bold: false, maxLines: 1, quiet: true };
      }
      return {
        size: "TEXT_SIZE_M",
        align,
        bold: leading,
        maxLines: layout === "FACE_LAYOUT_ICON_LABEL" || layout === undefined ? 2 : 1,
        quiet: false,
      };
    }
  }
}

/** The places worth offering in the editor for each layout, in the order it lists them. */
export const LAYOUT_SLOTS: Record<FaceLayout, readonly Slot[]> = {
  FACE_LAYOUT_ICON_LABEL: ["SLOT_ICON", "SLOT_TITLE", "SLOT_BADGE", "SLOT_VISUAL"],
  FACE_LAYOUT_VALUE: ["SLOT_TOP", "SLOT_VALUE", "SLOT_CAPTION", "SLOT_BADGE", "SLOT_VISUAL"],
  FACE_LAYOUT_ART: ["SLOT_TITLE", "SLOT_CAPTION", "SLOT_TOP", "SLOT_BADGE", "SLOT_VISUAL"],
  FACE_LAYOUT_SPLIT: ["SLOT_VISUAL", "SLOT_ICON", "SLOT_TITLE", "SLOT_CAPTION", "SLOT_TOP", "SLOT_BADGE"],
  FACE_LAYOUT_DIAL: ["SLOT_VISUAL", "SLOT_VALUE", "SLOT_CAPTION", "SLOT_BADGE"],
};

// ---- Clocks ----------------------------------------------------------------

/**
 * `at` as a clock element shows it: an strftime-style `format` ("%H:%M"),
 * in the IANA `zone` (empty: this machine's). The tablet's `Clock.kt`
 * understands the same codes. An unknown zone falls back to the local one.
 */
export function clockText(at: Date, format: string, zone: string, seconds: boolean, locale = "en-US"): string {
  const strftime = format.trim() || (seconds ? "%H:%M:%S" : "%H:%M");
  const options: Intl.DateTimeFormatOptions = { hourCycle: "h23" };
  try {
    if (zone.trim()) {
      new Intl.DateTimeFormat(locale, { timeZone: zone.trim() });
      options.timeZone = zone.trim();
    }
  } catch {
    // An unknown zone: this machine's, rather than a wrong hour.
  }
  const part = (extra: Intl.DateTimeFormatOptions, type: Intl.DateTimeFormatPartTypes): string =>
    new Intl.DateTimeFormat(locale, { ...options, ...extra }).formatToParts(at).find((p) => p.type === type)?.value ?? "";
  const hour24 = Number(part({ hour: "2-digit" }, "hour")) % 24;
  const hour12 = hour24 % 12 === 0 ? 12 : hour24 % 12;
  const pad = (n: number | string) => String(n).padStart(2, "0");
  const codes: Record<string, () => string> = {
    H: () => pad(hour24),
    "-H": () => String(hour24),
    I: () => pad(hour12),
    "-I": () => String(hour12),
    M: () => pad(part({ minute: "2-digit" }, "minute")),
    S: () => pad(part({ second: "2-digit" }, "second")),
    p: () => (hour24 < 12 ? "AM" : "PM"),
    a: () => part({ weekday: "short" }, "weekday"),
    A: () => part({ weekday: "long" }, "weekday"),
    d: () => pad(part({ day: "numeric" }, "day")),
    "-d": () => part({ day: "numeric" }, "day"),
    e: () => part({ day: "numeric" }, "day"),
    m: () => pad(part({ month: "numeric" }, "month")),
    "-m": () => part({ month: "numeric" }, "month"),
    b: () => part({ month: "short" }, "month"),
    B: () => part({ month: "long" }, "month"),
    Y: () => part({ year: "numeric" }, "year"),
    y: () => part({ year: "2-digit" }, "year"),
  };
  return strftime.replace(/%(-?[A-Za-z%])/g, (whole, code: string) => (code === "%" ? "%" : (codes[code]?.() ?? whole)));
}

/** Hour, minute and second hand angles in degrees clockwise from 12. */
export function clockHands(at: Date, zone: string): { hour: number; minute: number; second: number } {
  const text = clockText(at, "%H %M %S", zone, true).split(" ").map(Number);
  const [h = 0, m = 0, s = 0] = text;
  const second = s + at.getMilliseconds() / 1000;
  const minute = m + second / 60;
  return { hour: ((h % 12) + minute / 60) * 30, minute: minute * 6, second: second * 6 };
}
