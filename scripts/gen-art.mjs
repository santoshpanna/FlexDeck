/**
 * Render the README and docs artwork from the `[f]` mark.
 *
 * Same source as scripts/gen-brand.mjs (packages/ui/src/brand/mark.ts), so the
 * logo on GitHub is the logo in the apps. Writes plain SVG to .github/assets/:
 *
 *   logo.svg            the app-icon tile on its own
 *   banner-dark.svg     mark, wordmark, tagline and a deck — for dark GitHub
 *   banner-light.svg    the same on a light page (the deck stays dark)
 *   features-*.svg      the feature cards          dark and light, each
 *   how-*.svg           tablet, desktop, browser   carrying its own heading
 *   plugins-*.svg       a deck of plugin keys
 *   download-*.svg      store-badge download buttons
 *
 * Text uses the platform's monospace font: GitHub serves these through <img>,
 * which cannot load web fonts.
 *
 * Run: node scripts/gen-art.mjs   (or `pnpm brand:icons`, which runs both)
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { LEFT_BRACKET, LETTER_F, MARK_BOX, MARK_COLORS, RIGHT_BRACKET, TILE_RADIUS, partPath } from "../packages/ui/src/brand/mark.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const write = (name, content) => {
  const path = join(root, ".github", "assets", name);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
  return name;
};

const MONO = `ui-monospace,'SF Mono','JetBrains Mono','Cascadia Mono',Menlo,Consolas,monospace`;
const ORANGE = MARK_COLORS.letter;

/** The mark as a group, scaled to `size` px with its top-left at (x, y). */
function mark(x, y, size, outline = "none") {
  const k = size / MARK_BOX;
  return `<g transform="translate(${x} ${y}) scale(${k})">
    <rect width="${MARK_BOX}" height="${MARK_BOX}" rx="${MARK_BOX * TILE_RADIUS}" fill="${MARK_COLORS.tile}" stroke="${outline}" stroke-width="${1 / k}"/>
    <path fill="${MARK_COLORS.bracket}" d="${partPath([...LEFT_BRACKET, ...RIGHT_BRACKET])}"/>
    <path fill="${ORANGE}" d="${partPath(LETTER_F)}"/>
  </g>`;
}

const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

// --- the deck ----------------------------------------------------------------

/**
 * A tablet showing a deck: a 4×3 grid of keys, one 1×2 fader. `lit` keys glow
 * the signal orange (live state on), `led` keys carry a small status light.
 */
const KEYS = [
  { c: 0, r: 0, label: ["SCENE", "CAMERA"], led: "on" },
  { c: 1, r: 0, label: ["SCENE", "DESK"], led: "off" },
  { c: 2, r: 0, label: ["● REC", "00:42:17"], lit: true },
  { c: 3, r: 0, label: ["MIC", "LIVE"], led: "on" },
  { c: 0, r: 1, label: ["◂◂", "PREV"] },
  { c: 1, r: 1, label: ["▶ ❚❚", "PLAY"] },
  { c: 2, r: 1, label: ["CPU", "23%"], spark: true },
  { c: 3, r: 1, rows: 2, fader: 0.62, label: ["VOL"] },
  { c: 0, r: 2, label: ["DISCORD", "DEAFEN"], led: "off" },
  { c: 1, r: 2, label: ["LIGHTS", "WARM"] },
  { c: 2, r: 2, label: ["14:32", "SAT 11"] },
];

function deck(x, y) {
  const W = 470, H = 320, pad = 22;
  const cols = 4, rows = 3, gap = 10;
  const cw = (W - 2 * pad - (cols - 1) * gap) / cols;
  const ch = (H - 2 * pad - (rows - 1) * gap) / rows;

  const keys = KEYS.map((k) => {
    const kx = x + pad + k.c * (cw + gap);
    const ky = y + pad + k.r * (ch + gap);
    const kh = ch * (k.rows ?? 1) + gap * ((k.rows ?? 1) - 1);
    const parts = [];
    parts.push(`<rect x="${kx}" y="${ky}" width="${cw}" height="${kh}" rx="10" fill="${k.lit ? "#2a160d" : "#1c1c1f"}" stroke="${k.lit ? ORANGE : "#2c2c30"}" stroke-width="${k.lit ? 1.5 : 1}"/>`);
    parts.push(`<rect x="${kx + 1}" y="${ky + 1}" width="${cw - 2}" height="${kh / 2}" rx="9" fill="url(#keyshine)"/>`);
    if (k.lit) parts.push(`<rect x="${kx}" y="${ky}" width="${cw}" height="${kh}" rx="10" fill="url(#litglow)"/>`);

    if (k.fader !== undefined) {
      const tx = kx + cw / 2, t0 = ky + 34, t1 = ky + kh - 18;
      const knob = t1 - (t1 - t0) * k.fader;
      parts.push(`<line x1="${tx}" y1="${t0}" x2="${tx}" y2="${t1}" stroke="#34343a" stroke-width="4" stroke-linecap="round"/>`);
      parts.push(`<line x1="${tx}" y1="${knob}" x2="${tx}" y2="${t1}" stroke="${ORANGE}" stroke-width="4" stroke-linecap="round"/>`);
      parts.push(`<rect x="${tx - 18}" y="${knob - 7}" width="36" height="14" rx="4" fill="#ecebe6"/>`);
      parts.push(`<text x="${tx}" y="${ky + 22}" text-anchor="middle" font-family="${MONO}" font-size="12" font-weight="600" letter-spacing="1" fill="#8d8c86">VOL ${Math.round(k.fader * 100)}</text>`);
      return parts.join("\n    ");
    }

    const [top, bottom] = k.label;
    const cx = kx + cw / 2;
    parts.push(`<text x="${cx}" y="${ky + kh / 2 - 2}" text-anchor="middle" font-family="${MONO}" font-size="14" font-weight="700" letter-spacing="0.5" fill="${k.lit ? ORANGE : "#ecebe6"}">${esc(top)}</text>`);
    if (bottom) parts.push(`<text x="${cx}" y="${ky + kh / 2 + 17}" text-anchor="middle" font-family="${MONO}" font-size="11" letter-spacing="1" fill="${k.lit ? "#ffb08f" : "#8d8c86"}">${esc(bottom)}</text>`);
    if (k.led) parts.push(`<circle cx="${kx + cw - 12}" cy="${ky + 12}" r="3.5" fill="${k.led === "on" ? "#3ddc84" : "#3a3a3f"}"/>`);
    if (k.spark) {
      const pts = [9, 14, 11, 18, 13, 22, 16, 12, 19, 15, 23].map((v, i) => `${kx + 14 + i * ((cw - 28) / 10)},${ky + kh - 10 - v * 0.5}`).join(" ");
      parts.push(`<polyline points="${pts}" fill="none" stroke="${ORANGE}" stroke-width="1.5" stroke-linejoin="round" opacity="0.8"/>`);
    }
    return parts.join("\n    ");
  });

  return `<g>
    <rect x="${x - 16}" y="${y - 16}" width="${W + 32}" height="${H + 32}" rx="30" fill="#0e0e10" stroke="#2a2a2e"/>
    <rect x="${x - 15}" y="${y - 15}" width="${W + 30}" height="${H + 30}" rx="29" fill="none" stroke="#ffffff" stroke-opacity="0.04"/>
    <circle cx="${x + W + 6}" cy="${y + H / 2}" r="2.5" fill="#26262a"/>
    <rect x="${x}" y="${y}" width="${W}" height="${H}" rx="14" fill="#111113"/>
    ${keys.join("\n    ")}
  </g>`;
}

// --- banner ------------------------------------------------------------------

function banner({ bg, dot, ink, muted, glow, outline }) {
  const W = 1280, H = 440;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-label="flexdeck — every key, exactly yours">
  <defs>
    <pattern id="dots" width="20" height="20" patternUnits="userSpaceOnUse">
      <circle cx="10" cy="10" r="1" fill="${dot}"/>
    </pattern>
    <radialGradient id="halo" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0" stop-color="${ORANGE}" stop-opacity="${glow}"/>
      <stop offset="1" stop-color="${ORANGE}" stop-opacity="0"/>
    </radialGradient>
    <linearGradient id="keyshine" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#ffffff" stop-opacity="0.05"/>
      <stop offset="1" stop-color="#ffffff" stop-opacity="0"/>
    </linearGradient>
    <radialGradient id="litglow" cx="0.5" cy="0.5" r="0.7">
      <stop offset="0" stop-color="${ORANGE}" stop-opacity="0.22"/>
      <stop offset="1" stop-color="${ORANGE}" stop-opacity="0"/>
    </radialGradient>
    <linearGradient id="fade" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="${bg}" stop-opacity="0.9"/>
      <stop offset="0.5" stop-color="${bg}" stop-opacity="0.2"/>
      <stop offset="1" stop-color="${bg}" stop-opacity="0"/>
    </linearGradient>
  </defs>
  <rect width="${W}" height="${H}" rx="24" fill="${bg}"/>
  <rect width="${W}" height="${H}" rx="24" fill="url(#dots)"/>
  <rect width="${W}" height="${H}" rx="24" fill="url(#fade)"/>
  <ellipse cx="965" cy="220" rx="380" ry="230" fill="url(#halo)"/>

  ${mark(84, 112, 88, outline)}
  <text x="196" y="178" font-family="${MONO}" font-size="68" font-weight="600" letter-spacing="-2" fill="${ink}">flexdeck</text>

  <text x="86" y="262" font-family="${MONO}" font-size="30" font-weight="500" letter-spacing="-0.5" fill="${ink}">Every key, exactly yours.</text>
  <text x="86" y="300" font-family="${MONO}" font-size="16" fill="${muted}">Turn the tablet in your drawer into a control surface.</text>

  <g font-family="${MONO}" font-size="12" font-weight="600" letter-spacing="2" fill="${muted}">
    <circle cx="90" cy="351" r="3.5" fill="${ORANGE}"/><text x="102" y="355">LAN ONLY</text>
    <circle cx="210" cy="351" r="3.5" fill="${ORANGE}"/><text x="222" y="355">TLS PINNED</text>
    <circle cx="346" cy="351" r="3.5" fill="${ORANGE}"/><text x="358" y="355">NO ACCOUNTS</text>
    <circle cx="492" cy="351" r="3.5" fill="${ORANGE}"/><text x="504" y="355">OPEN PLUGINS</text>
  </g>

  ${deck(744, 60)}
</svg>
`;
}

const logo = `<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128" role="img" aria-label="flexdeck">
  ${mark(0, 0, 128)}
</svg>
`;

// --- section panels ----------------------------------------------------------

/**
 * README section panels. Each one carries its own heading, so the Markdown
 * around them stays short. Keys are always dark: the deck never follows the
 * page's theme, the same rule the apps follow.
 */
const THEMES = {
  dark: { bg: "#0b0b0c", card: "#131315", line: "#26262a", ink: MARK_COLORS.ink, muted: "#8d8c86", dot: "#ffffff12" },
  light: { bg: "#f6f5f1", card: "#ffffff", line: "#e3e1da", ink: "#111113", muted: "#6b6a65", dot: "#11111317" },
};
const KEY = { face: "#1c1c1f", edge: "#2c2c30", ink: MARK_COLORS.ink, muted: "#8d8c86" };

const defs = (t) => `<defs>
    <pattern id="dots" width="20" height="20" patternUnits="userSpaceOnUse"><circle cx="10" cy="10" r="1" fill="${t.dot}"/></pattern>
    <linearGradient id="keyshine" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#ffffff" stop-opacity="0.06"/><stop offset="1" stop-color="#ffffff" stop-opacity="0"/>
    </linearGradient>
    <radialGradient id="litglow" cx="0.5" cy="0.5" r="0.7">
      <stop offset="0" stop-color="${ORANGE}" stop-opacity="0.24"/><stop offset="1" stop-color="${ORANGE}" stop-opacity="0"/>
    </radialGradient>
  </defs>`;

const text = (x, y, str, { size = 14, weight = 400, fill, anchor = "start", spacing = 0 } = {}) =>
  `<text x="${x}" y="${y}" font-family="${MONO}" font-size="${size}" font-weight="${weight}" letter-spacing="${spacing}" text-anchor="${anchor}" fill="${fill}">${esc(str)}</text>`;

/** A panel: page background, dot grid, and the `// label` + title heading. */
function panel(t, { w, h, label, title, aria }, body) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" role="img" aria-label="${esc(aria)}">
  ${defs(t)}
  <rect width="${w}" height="${h}" rx="24" fill="${t.bg}"/>
  <rect width="${w}" height="${h}" rx="24" fill="url(#dots)"/>
  ${text(40, 58, `// ${label}`, { size: 13, weight: 600, fill: ORANGE, spacing: 2.5 })}
  ${text(40, 98, title, { size: 30, weight: 600, fill: t.ink, spacing: -0.5 })}
  ${body}
</svg>
`;
}

/** A dark key, `lit` glowing orange; `dashed` is an empty slot. */
function key(x, y, w, h, { lit = false, dashed = false, rx = 12 } = {}) {
  if (dashed) return `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${rx}" fill="none" stroke="${ORANGE}" stroke-width="1.5" stroke-dasharray="6 5"/>`;
  const glow = lit ? `\n    <rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${rx}" fill="url(#litglow)"/>` : "";
  return `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${rx}" fill="${lit ? "#2a160d" : KEY.face}" stroke="${lit ? ORANGE : KEY.edge}" stroke-width="${lit ? 1.5 : 1}"/>
    <rect x="${x + 1}" y="${y + 1}" width="${w - 2}" height="${h / 2}" rx="${rx - 1}" fill="url(#keyshine)"/>${glow}`;
}

// Line-art icons, drawn on a 72-unit key face.
const STROKE = `fill="none" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"`;
const O = ORANGE, K = KEY.ink;
const ICONS = {
  gestures: `<circle cx="36" cy="36" r="5" fill="${O}"/><circle cx="36" cy="36" r="12" ${STROKE} stroke="${K}" opacity="0.55"/><circle cx="36" cy="36" r="19" ${STROKE} stroke="${K}" opacity="0.25"/>`,
  looks: `<rect x="17" y="17" width="38" height="38" rx="8" ${STROKE} stroke="${K}" opacity="0.7"/><circle cx="46" cy="26" r="10" fill="${O}" opacity="0.25"/><circle cx="46" cy="26" r="4" fill="${O}"/><path d="M25 46h14" ${STROKE} stroke="${K}" opacity="0.7"/>`,
  faders: `<path d="M27 16v40M45 16v40" ${STROKE} stroke="${K}" opacity="0.3"/><path d="M27 38v18M45 26v30" ${STROKE} stroke="${O}"/><rect x="20" y="33" width="14" height="8" rx="2" fill="${K}"/><rect x="38" y="21" width="14" height="8" rx="2" fill="${K}"/>`,
  flows: `<path d="M22 22L50 36L22 50" ${STROKE} stroke="${K}" opacity="0.5"/><circle cx="22" cy="22" r="5" fill="${KEY.face}" stroke="${K}" stroke-width="2.5"/><circle cx="22" cy="50" r="5" fill="${KEY.face}" stroke="${K}" stroke-width="2.5"/><circle cx="50" cy="36" r="6" fill="${O}"/>`,
  plugins: `<rect x="17" y="17" width="16" height="16" rx="3" ${STROKE} stroke="${K}" opacity="0.7"/><rect x="39" y="17" width="16" height="16" rx="3" ${STROKE} stroke="${K}" opacity="0.7"/><rect x="17" y="39" width="16" height="16" rx="3" ${STROKE} stroke="${K}" opacity="0.7"/><rect x="40" y="40" width="16" height="16" rx="3" fill="${O}"/>`,
  grid: `<rect x="16" y="16" width="11" height="11" rx="2" fill="${K}" opacity="0.5"/><rect x="31" y="16" width="26" height="11" rx="2" fill="${O}"/><rect x="16" y="31" width="11" height="11" rx="2" fill="${K}" opacity="0.5"/><rect x="31" y="31" width="11" height="11" rx="2" fill="${K}" opacity="0.5"/><rect x="46" y="31" width="11" height="26" rx="2" fill="${K}" opacity="0.5"/><rect x="16" y="46" width="26" height="11" rx="2" fill="${K}" opacity="0.5"/>`,
  lock: `<path d="M27 33v-6a9 9 0 0 1 18 0v6" ${STROKE} stroke="${O}"/><rect x="21" y="33" width="30" height="22" rx="5" ${STROKE} stroke="${K}" opacity="0.8"/><circle cx="36" cy="44" r="3" fill="${O}"/>`,
  pin: `<path d="M18 24h36M18 50h36" ${STROKE} stroke="${K}" opacity="0.3" stroke-dasharray="4 4"/>${text(36, 42, "482", { size: 16, weight: 700, fill: O, anchor: "middle", spacing: 1 })}`,
};

const FEATURES = [
  ["gestures", "01 · KEYCAP", "Four gestures per key", ["Tap, double tap, long press and", "hold. Each runs its own flow."]],
  ["looks", "02 · LOOKS", "Keys that react", ["Looks follow live values:", "recording, muted, now playing."]],
  ["faders", "03 · FADERS", "Faders that feel physical", ["Paced so your thumb never fights", "the echo. Graceful on release."]],
  ["flows", "04 · FLOWS", "Automations without code", ["When, if, repeat, do: in plain", "sentences. Dry-run them first."]],
  ["plugins", "05 · PLUGINS", "An open plugin system", ["OBS, Discord, Spotify and more.", "Every permission shown first."]],
  ["grid", "06 · PLATE", "Any grid, any tablet", ["Reflows to the screen. Preview", "and tablet match to the pixel."]],
  ["lock", "07 · LINK", "Your Wi-Fi only", ["Tablets talk to your PC over", "pinned TLS. No cloud in the loop."]],
  ["pin", "08 · PAIRING", "No accounts, ever", ["Nothing to sign up for. Pair", "with six digits and you're done."]],
];

function features(t) {
  const W = 960, x0 = 40, y0 = 132, gap = 16, cw = (W - 2 * x0 - gap) / 2, ch = 112;
  const cards = FEATURES.map(([icon, tag, title, body], i) => {
    const x = x0 + (i % 2) * (cw + gap), y = y0 + Math.floor(i / 2) * (ch + gap);
    return `<g>
    <rect x="${x}" y="${y}" width="${cw}" height="${ch}" rx="16" fill="${t.card}" stroke="${t.line}"/>
    ${key(x + 20, y + 20, 72, 72, { rx: 14 })}
    <g transform="translate(${x + 20} ${y + 20})">${ICONS[icon]}</g>
    ${text(x + 112, y + 34, tag, { size: 11, weight: 600, fill: t.muted, spacing: 2 })}
    ${text(x + 112, y + 58, title, { size: 18, weight: 700, fill: t.ink })}
    ${text(x + 112, y + 80, body[0], { size: 13.5, fill: t.muted })}
    ${text(x + 112, y + 99, body[1], { size: 13.5, fill: t.muted })}
  </g>`;
  });
  const H = y0 + 4 * ch + 3 * gap + 40;
  return panel(t, { w: W, h: H, label: "FEATURES", title: "Built like hardware.", aria: "Features: " + FEATURES.map((f) => f[2]).join(", ") }, cards.join("\n  "));
}

/** Tablet, desktop and browser, with traffic flowing along the links. */
function howItWorks(t) {
  const W = 960, H = 430;
  const flow = (x1, x2, y, [top, bottom]) => `
    <line x1="${x1}" y1="${y}" x2="${x2}" y2="${y}" stroke="${t.line}" stroke-width="2"/>
    <line x1="${x1}" y1="${y}" x2="${x2}" y2="${y}" stroke="${ORANGE}" stroke-width="2" stroke-dasharray="3 9" stroke-linecap="round">
      <animate attributeName="stroke-dashoffset" from="24" to="0" dur="0.8s" repeatCount="indefinite"/>
    </line>
    ${text((x1 + x2) / 2, y - 28, top, { size: 10, weight: 700, fill: t.muted, anchor: "middle", spacing: 1.5 })}
    ${text((x1 + x2) / 2, y - 14, bottom, { size: 10, weight: 600, fill: t.muted, anchor: "middle", spacing: 1.5 })}`;

  // Tablet: a small deck, one key lit and its light pulsing.
  const tx = 72, ty = 150, tw = 196, th = 128;
  const tkeys = [];
  for (let r = 0; r < 2; r++) for (let c = 0; c < 3; c++) tkeys.push(key(tx + 16 + c * 58, ty + 16 + r * 50, 50, 42, { lit: r === 0 && c === 1, rx: 8 }));
  const tablet = `
    <rect x="${tx - 10}" y="${ty - 10}" width="${tw + 20}" height="${th + 20}" rx="20" fill="#0e0e10" stroke="#2a2a2e"/>
    <rect x="${tx}" y="${ty}" width="${tw}" height="${th}" rx="10" fill="#111113"/>
    ${tkeys.join("\n    ")}
    <circle cx="${tx + 116}" cy="${ty + 24}" r="3" fill="${ORANGE}"><animate attributeName="opacity" values="1;0.25;1" dur="1.6s" repeatCount="indefinite"/></circle>
    ${text(tx + 41, ty + 41, "CAM", { size: 10, weight: 700, fill: KEY.ink, anchor: "middle" })}
    ${text(tx + 99, ty + 41, "REC", { size: 10, weight: 700, fill: ORANGE, anchor: "middle" })}
    ${text(tx + 157, ty + 41, "BRB", { size: 10, weight: 700, fill: KEY.ink, anchor: "middle" })}
    ${text(tx + 41, ty + 92, "MUTE", { size: 10, weight: 700, fill: KEY.ink, anchor: "middle" })}
    ${text(tx + 99, ty + 92, "SCENE", { size: 10, weight: 700, fill: KEY.ink, anchor: "middle" })}
    ${text(tx + 157, ty + 92, "PLAY", { size: 10, weight: 700, fill: KEY.ink, anchor: "middle" })}`;

  // Desktop: a monitor showing the mark.
  const mx = 382, my = 146, mw = 196, mh = 122;
  const monitor = `
    <rect x="${mx}" y="${my}" width="${mw}" height="${mh}" rx="12" fill="#111113" stroke="#2a2a2e"/>
    ${mark(mx + mw / 2 - 26, my + 20, 52, "#2c2c30")}
    ${text(mx + mw / 2, my + 98, "flexdeck", { size: 14, weight: 600, fill: KEY.ink, anchor: "middle" })}
    <path d="M${mx + mw / 2 - 14} ${my + mh}l-6 18h40l-6-18" fill="#1a1a1d" stroke="#2a2a2e"/>
    <rect x="${mx + mw / 2 - 38}" y="${my + mh + 18}" width="76" height="5" rx="2.5" fill="#2a2a2e"/>`;

  // Browser: the editor, one key selected.
  const bx = 692, by = 150, bw = 196, bh = 128;
  const bkeys = [];
  for (let r = 0; r < 2; r++) for (let c = 0; c < 4; c++) bkeys.push(`<rect x="${bx + 18 + c * 41}" y="${by + 46 + r * 38}" width="35" height="32" rx="6" fill="${KEY.face}" stroke="${KEY.edge}"/>`);
  const browser = `
    <rect x="${bx}" y="${by}" width="${bw}" height="${bh}" rx="12" fill="#111113" stroke="#2a2a2e"/>
    <path d="M${bx} ${by + 30}h${bw}" stroke="#2a2a2e"/>
    <circle cx="${bx + 16}" cy="${by + 15}" r="3.5" fill="#3a3a3f"/><circle cx="${bx + 28}" cy="${by + 15}" r="3.5" fill="#3a3a3f"/><circle cx="${bx + 40}" cy="${by + 15}" r="3.5" fill="#3a3a3f"/>
    <rect x="${bx + 56}" y="${by + 7}" width="${bw - 70}" height="16" rx="8" fill="#1c1c1f"/>
    ${text(bx + 56 + (bw - 70) / 2, by + 19, "flex-deck.com", { size: 10, fill: KEY.muted, anchor: "middle" })}
    ${bkeys.join("\n    ")}
    <rect x="${bx + 56}" y="${by + 43}" width="41" height="38" rx="8" fill="none" stroke="${ORANGE}" stroke-width="1.5" stroke-dasharray="4 3"/>`;

  const step = (cx, n, title, sub) => `
    ${text(cx, 330, n, { size: 12, weight: 700, fill: ORANGE, anchor: "middle", spacing: 2 })}
    ${text(cx, 356, title, { size: 18, weight: 700, fill: t.ink, anchor: "middle" })}
    ${text(cx, 378, sub, { size: 13.5, fill: t.muted, anchor: "middle" })}`;

  const aria = "How it works: the tablet talks to the desktop app over your Wi-Fi with pinned TLS; you edit the deck in a browser, directly on the same PC or through the relay.";
  return panel(t, { w: W, h: H, label: "HOW IT WORKS", title: "Three pieces. No ports to open.", aria }, `
    ${flow(tx + tw + 14, mx - 12, 212, ["LAN", "PINNED TLS"])}
    ${flow(mx + mw + 12, bx - 14, 212, ["LOOPBACK", "OR RELAY"])}
    ${tablet}
    ${monitor}
    ${browser}
    ${step(tx + tw / 2, "02 · PAIR", "Pair the tablet", "six digits, once")}
    ${step(mx + mw / 2, "01 · INSTALL", "Run the desktop app", "it lives in the tray")}
    ${step(bx + bw / 2, "03 · EDIT", "Build in a browser", "the deck updates live")}
    ${text(W / 2, 412, "The desktop dials out. Nothing dials in.", { size: 12, weight: 600, fill: t.muted, anchor: "middle", spacing: 1 })}`);
}

const PLUGINS = [
  { name: "OBS STUDIO", act: "● REC  00:42", lit: true },
  { name: "DISCORD", act: "PUSH TO TALK", led: true },
  { name: "SPOTIFY", act: "♥ LIKED" },
  { name: "STREAMER.BOT", act: "RUN ACTION" },
  { name: "SOUNDBOARD", act: "AIRHORN" },
  { name: "WEATHER", act: "24° · CLEAR" },
  { name: "HOME ASSISTANT", act: "LIGHTS ON", led: true },
  { name: "+ YOUR PLUGIN", act: "TS · PYTHON · RUST", yours: true },
];

/** A deck of plugin keys. */
function plugins(t) {
  const W = 960, px = 40, py = 132, pw = W - 2 * px, pad = 18, gap = 14, kh = 92;
  const kw = (pw - 2 * pad - 3 * gap) / 4;
  const keys = PLUGINS.map((p, i) => {
    const x = px + pad + (i % 4) * (kw + gap), y = py + pad + Math.floor(i / 4) * (kh + gap);
    const cx = x + kw / 2;
    const led = p.led ? `\n    <circle cx="${x + kw - 13}" cy="${y + 13}" r="3.5" fill="#3ddc84"/>` : "";
    return `${key(x, y, kw, kh, { lit: p.lit, dashed: p.yours })}
    ${text(cx, y + 42, p.name, { size: 14, weight: 700, fill: p.lit || p.yours ? ORANGE : KEY.ink, anchor: "middle", spacing: 0.5 })}
    ${text(cx, y + 64, p.act, { size: 11.5, fill: p.lit ? "#ffb08f" : KEY.muted, anchor: "middle", spacing: 1 })}${led}`;
  });
  const ph = 2 * pad + 2 * kh + gap;
  return panel(t, { w: W, h: py + ph + 40, label: "PLUGINS", title: "Plays well with your setup.", aria: "Plugins: " + PLUGINS.map((p) => p.name).join(", ") }, `
    <rect x="${px}" y="${py}" width="${pw}" height="${ph}" rx="20" fill="#0e0e10" stroke="#2a2a2e"/>
    ${keys.join("\n    ")}`);
}

/** A store-badge-shaped download button, the same on light and dark pages. */
function button({ w = 232, top, big, primary, icon }) {
  const h = 60;
  const fg = primary ? "#111113" : MARK_COLORS.ink;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" role="img" aria-label="${esc(`${top} ${big}`)}">
  <rect x="0.5" y="0.5" width="${w - 1}" height="${h - 1}" rx="14" fill="${primary ? ORANGE : "#111113"}" stroke="${primary ? ORANGE : "#2c2c30"}"/>
  <g transform="translate(20 15)">${icon(fg)}</g>
  ${text(66, 26, top, { size: 10, weight: 600, fill: primary ? "#3a1a0c" : "#8d8c86", spacing: 1.5 })}
  ${text(66, 45, big, { size: 19, weight: 700, fill: fg, spacing: -0.3 })}
</svg>
`;
}

const windowsIcon = (c) => `<path fill="${c}" d="M0 4.2l12-1.7v11.6H0zM13.5 2.3L30 0v14.1H13.5zM0 15.9h12v11.6L0 25.8zM13.5 15.9H30V30l-16.5-2.3z"/>`;
const androidIcon = () => `<path fill="#3ddc84" d="M2 22a13 13 0 0 1 26 0z"/><path d="M7 7.5l3 5M23 7.5l-3 5" stroke="#3ddc84" stroke-width="2" stroke-linecap="round"/><rect x="2" y="24" width="26" height="6" rx="2" fill="#3ddc84"/><circle cx="10" cy="17" r="1.8" fill="#111113"/><circle cx="20" cy="17" r="1.8" fill="#111113"/>`;

const written = [
  write("logo.svg", logo),
  write("banner-dark.svg", banner({ bg: "#0b0b0c", dot: "#ffffff1a", ink: MARK_COLORS.ink, muted: "#8d8c86", glow: 0.16, outline: "#2c2c30" })),
  write("banner-light.svg", banner({ bg: "#f6f5f1", dot: "#1111131f", ink: "#111113", muted: "#6b6a65", glow: 0.12, outline: "none" })),
  ...Object.entries(THEMES).flatMap(([name, theme]) => [
    write(`features-${name}.svg`, features(theme)),
    write(`how-${name}.svg`, howItWorks(theme)),
    write(`plugins-${name}.svg`, plugins(theme)),
  ]),
  write("download-windows.svg", button({ top: "DOWNLOAD FOR", big: "Windows", primary: true, icon: windowsIcon })),
  write("download-android.svg", button({ top: "DOWNLOAD THE APK", big: "Android", primary: false, icon: androidIcon })),
];

console.log(`wrote .github/assets/:\n  ${written.join("\n  ")}`);
