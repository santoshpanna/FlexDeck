/**
 * Contrast audit for the flexdeck palette.
 *
 *   node packages/tokens/scripts/check-contrast.mjs
 *
 * The palette is READ from `src/tokens.css`, not restated here: one home for the
 * values, and the measurement cannot go stale. `src/tokens.ts` is the same
 * palette in a notation a canvas can use, so it is checked against the CSS too —
 * two representations that are verified against each other cannot drift.
 *
 * Every pair below is one the stylesheets actually render: a foreground against
 * the background it really sits on, not against the page. Contrast is the one
 * interface concern with an exact answer, so it is computed rather than
 * eyeballed.
 *
 * Reports APCA (Lc, the better model for design decisions) and the WCAG 2 ratio
 * (which still has the legal standing), against the thresholds in the
 * `better-colors` skill.
 *
 * Exits non-zero when a GATED pair fails, or when the two notations disagree. A
 * pair is gated when the interface would be broken rather than merely plain —
 * everything else is printed with its measurement and left alone, because
 * changing a colour is a design decision.
 */

import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const source = join(dirname(fileURLToPath(import.meta.url)), "..", "src");

/**
 * Pull `--name: value` out of the two declaration blocks in `tokens.css`.
 *
 * Comments come out first, or a trailing comment becomes part of a value. The
 * `:root.dark` alternative is tried first so it is not mistaken for `:root`.
 */
function readPalette(css) {
  const clean = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const blocks = {};
  const block = /(:root(?:\.dark)?)\s*\{([^}]*)\}/g;
  for (let match = block.exec(clean); match !== null; match = block.exec(clean)) {
    blocks[match[1]] = match[2];
  }

  const declarations = (text) => {
    const values = {};
    const declaration = /--([a-z0-9-]+)\s*:\s*([^;]+);/g;
    for (let match = declaration.exec(text); match !== null; match = declaration.exec(text)) {
      values[match[1]] = match[2].trim();
    }
    return values;
  };

  return {
    light: declarations(blocks[":root"] ?? ""),
    dark: declarations(blocks[":root.dark"] ?? ""),
  };
}

/** Only the colours. Fonts, radii, tracking and the shadows are not tokens a
 *  contrast pair can be measured from. */
const isColour = (value) => /^#[0-9a-fA-F]{3,8}$/.test(value);
const coloursOnly = (values) =>
  Object.fromEntries(Object.entries(values).filter(([, value]) => isColour(value)));

const cssText = await readFile(join(source, "tokens.css"), "utf8");
const parsed = readPalette(cssText);

/** The light block IS the base: a token dark does not redefine keeps its value,
 *  exactly as the cascade resolves it. */
export const LIGHT = coloursOnly(parsed.light);
export const DARK = { ...LIGHT, ...coloursOnly(parsed.dark) };

// --- the two notations must agree -----------------------------------------

const typescript = await import(pathToFileURL(join(source, "tokens.ts")).href);

/** Compare one notation against the stylesheet from both directions, so neither
 *  a missing token nor an extra one can hide. */
function compareNotation(name, declared, fromCss) {
  const problems = [];
  for (const [token, value] of Object.entries(declared)) {
    if (!(token in fromCss)) {
      problems.push(`${name}.${token} is not a colour in tokens.css`);
    } else if (fromCss[token].toLowerCase() !== value.toLowerCase()) {
      problems.push(`${name}.${token} is ${value}, tokens.css says ${fromCss[token]}`);
    }
  }
  for (const token of Object.keys(fromCss)) {
    if (!(token in declared)) {
      problems.push(`${name} is missing ${token}`);
    }
  }
  return problems;
}

const drift = [
  ...compareNotation("tokens", typescript.tokens, LIGHT),
  ...compareNotation("darkTokens", typescript.darkTokens, coloursOnly(parsed.dark)),
];

// --- which pairs actually render ------------------------------------------

/**
 * `on` names the token that paints the background. Every entry is a real pair
 * from `apps/portal/src/styles.css` or `apps/desktop/src/styles.css`; the
 * `where` column names the rule so the pair can be found again.
 *
 * `kind` picks the threshold: body | nonbody | large | ui.
 */
export const PAIRS = [
  { fg: "ink", bg: "paper", kind: "body", where: "body text" },
  { fg: "ink", bg: "surface", kind: "body", where: "card text on the panel" },
  // A link is a label, not a block of text: the accent is on nav/footer links
  // and on `.link` actions, never on a column of prose.
  { fg: "accent", bg: "paper", kind: "label", where: "a / .link text" },
  { fg: "accent", bg: "surface", kind: "label", where: "a inside a Card" },
  // `.muted` is the secondary/label colour (headings, .facts dt, .row-detail).
  // It cannot reach Lc 75 on a near-black ground without collapsing into
  // `--ink-2`, which is exactly why APCA has a separate non-body threshold.
  { fg: "muted", bg: "paper", kind: "label", where: ".muted" },
  { fg: "muted", bg: "surface", kind: "label", where: ".muted inside a Card" },
  { fg: "ink-2", bg: "paper", kind: "label", where: ".quiet button label" },
  { fg: "ink-2", bg: "surface", kind: "label", where: ".quiet button on a Card" },
  { fg: "faint", bg: "surface", kind: "label", where: ".badge.off (10px)" },
  { fg: "paper", bg: "ink", kind: "label", where: "primary button label" },
  { fg: "paper", bg: "ink-2", kind: "label", where: "primary button :hover" },
  { fg: "ink", bg: "surface-sunken", kind: "label", where: ".quiet :hover / tab :hover" },
  { fg: "bad", bg: "bad-soft", kind: "label", where: ".error text on its wash" },
  { fg: "warn", bg: "warn-soft", kind: "label", where: ".warn text on its wash" },
  { fg: "ok", bg: "surface", kind: "label", where: ".badge.ok" },
  { fg: "warn", bg: "surface", kind: "label", where: ".badge.warn" },
  { fg: "rule", bg: "surface", kind: "ui", where: "card / input outline" },
  { fg: "rule", bg: "paper", kind: "ui", where: "divider, input outline" },
  { fg: "accent", bg: "paper", kind: "ui", where: "focus ring on the page" },
  { fg: "accent", bg: "deck", kind: "label", where: ".preview-ghost dashes (over the device)" },
  { fg: "accent-soft", bg: "deck", kind: "ui", where: ".preview-ghost fill (over the device)" },
  { fg: "deck-ink", bg: "deck-widget", kind: "body", where: "widget label" },
  { fg: "deck-muted", bg: "deck", kind: "label", where: ".pager-count" },
  { fg: "deck-control-ink", bg: "deck-control", kind: "ui", where: ".widget-action glyph" },
  { fg: "deck-control", bg: "deck", kind: "ui", where: ".widget-action chip" },
];

/**
 * The ramp as DESIGNED — one held hue, chroma rising to the middle and falling
 * off at both ends, spaced on perceived lightness. `--plan` turns these into hex
 * so the design intent is visible next to the values it produced; `DARK` above
 * stays the hex record, because hex is the notation this project uses.
 */
export const PLAN = {
  neutrals: [
    ["surface-sunken", 0.155, 0.006],
    ["paper", 0.19, 0.007],
    ["surface", 0.235, 0.009],
    ["rule", 0.36, 0.016],
    ["faint", 0.62, 0.022],
    ["muted", 0.775, 0.024],
    ["ink-2", 0.855, 0.018],
    ["ink", 0.955, 0.012],
  ],
  // Hue is held per ramp: the brick accent stays at ~35, danger ~25, warning
  // ~85, success ~150 — the same relationships the light palette uses.
  hued: [
    ["accent", 0.8, 0.095, 35],
    ["accent-soft", 0.28, 0.05, 35],
    ["accent-ink", 0.19, 0.007, 35],
    ["bad", 0.8, 0.105, 25],
    ["bad-soft", 0.27, 0.05, 25],
    ["warn", 0.85, 0.11, 85],
    ["warn-soft", 0.29, 0.045, 85],
    ["ok", 0.82, 0.085, 150],
    ["ok-soft", 0.24, 0.035, 150],
    ["deck-bezel", 0.34, 0.02, 255],
  ],
};

// --- colour maths (no dependencies) --------------------------------------

const clamp = (n) => Math.min(1, Math.max(0, n));

function hexToRgb(hex) {
  const value = hex.replace("#", "");
  const full =
    value.length === 3
      ? value
          .split("")
          .map((c) => c + c)
          .join("")
      : value;
  return [0, 2, 4].map((i) => parseInt(full.slice(i, i + 2), 16) / 255);
}

export function toHex([r, g, b]) {
  return (
    "#" +
    [r, g, b]
      .map((c) =>
        Math.round(clamp(c) * 255)
          .toString(16)
          .padStart(2, "0"),
      )
      .join("")
  );
}

const srgbToLinear = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const linearToSrgb = (c) => (c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055);

/** WCAG 2 relative luminance the way the spec defines it. */
function luminance([r, g, b]) {
  return 0.2126 * srgbToLinear(r) + 0.7152 * srgbToLinear(g) + 0.0722 * srgbToLinear(b);
}

/** WCAG 2 contrast ratio, 1..21. */
export function wcag(fg, bg) {
  const a = luminance(hexToRgb(fg));
  const b = luminance(hexToRgb(bg));
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

/**
 * APCA-W3. Screen luminance is a plain 2.4 power here, not the sRGB piecewise
 * curve — that difference is deliberate in the algorithm.
 */
function apcaY(hex) {
  const [r, g, b] = hexToRgb(hex);
  const p = (c) => Math.sign(c) * Math.abs(c) ** 2.4;
  const y = 0.2126729 * p(r) + 0.7151522 * p(g) + 0.072175 * p(b);
  return y < 0.022 ? y + (0.022 - y) ** 1.414 : y;
}

/** APCA lightness contrast, signed: positive = dark on light. */
export function apca(fg, bg) {
  const txt = apcaY(fg);
  const back = apcaY(bg);
  if (Math.abs(back - txt) < 0.0005) return 0;
  if (back > txt) {
    const sapc = (back ** 0.56 - txt ** 0.57) * 1.14;
    return (sapc < 0.1 ? 0 : sapc - 0.027) * 100;
  }
  const sapc = (back ** 0.65 - txt ** 0.62) * 1.14;
  return (sapc > -0.1 ? 0 : sapc + 0.027) * 100;
}

/** OKLCH, for auditing the ramp itself rather than a rendered pair. */
export function oklch(hex) {
  const [r, g, b] = hexToRgb(hex).map(srgbToLinear);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const bb = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  const C = Math.hypot(a, bb);
  const H = ((Math.atan2(bb, a) * 180) / Math.PI + 360) % 360;
  return { L, C, H };
}

/** OKLCH -> hex, for designing a ramp by its perceived lightness. */
export function fromOklch(L, C, H) {
  const h = (H * Math.PI) / 180;
  const a = C * Math.cos(h);
  const b = C * Math.sin(h);
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const rgb = [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ].map((c) => linearToSrgb(c));
  // A value outside sRGB is not renderable; report it as clipped rather than
  // silently emitting a colour the browser will change.
  const clipped = rgb.some((c) => c < -0.0005 || c > 1.0005);
  return { hex: toHex(rgb), clipped };
}

// --- thresholds -----------------------------------------------------------

const THRESHOLDS = {
  // Blocks of prose.
  body: { lc: 75, ratio: 4.5, wcag: "AA" },
  // Labels, button text and headlines: still below 24px, so WCAG AA applies.
  label: { lc: 60, ratio: 4.5, wcag: "AA" },
  large: { lc: 45, ratio: 3, wcag: "AA large" },
  // Borders, rules and any non-text indicator.
  ui: { lc: 30, ratio: 3, wcag: "AA non-text" },
};

// --- the report -----------------------------------------------------------

function resolve(name, theme) {
  const value = theme[name];
  if (value === undefined) {
    throw new Error(`no such colour: ${name}`);
  }
  return value;
}

/** Gated = the interface breaks without it. Hairlines and the faint badge are
 *  reported because they are measured, but they are a design decision. */
const GATED = new Set([
  "body text",
  "card text on the panel",
  "a / .link text",
  "a inside a Card",
  ".muted",
  ".muted inside a Card",
  ".quiet button label",
  ".quiet button on a Card",
  "primary button label",
  "primary button :hover",
  ".quiet :hover / tab :hover",
  ".error text on its wash",
  ".warn text on its wash",
  "widget label",
  ".widget-action chip",
  ".widget-action glyph",
]);

/**
 * Pairs that already fail in the SHIPPED light palette. They are gated so the
 * report does not go quiet about them, but the failure is pre-existing and no
 * colour is changed to silence it: a failing pair is reported, not repainted.
 */
export const KNOWN_LIGHT_FAILURES = new Set([
  ".muted", // #777777 on #fafaf0 = 4.26:1, under AA
  ".muted inside a Card", // 3.99:1, under AA
  ".warn text on its wash", // 4.24:1, under AA
  ".preview-ghost dashes (over the device)", // 2.40:1 on the device's own screen
]);

export function audit(themeName, theme) {
  const rows = [];
  let gatedFailures = 0;

  for (const pair of PAIRS) {
    const fg = resolve(pair.fg, theme);
    const bg = resolve(pair.bg, theme);
    const lc = apca(fg, bg);
    const ratio = wcag(fg, bg);
    const limit = THRESHOLDS[pair.kind];
    const passes = Math.abs(lc) >= limit.lc && ratio >= limit.ratio;
    const gated = GATED.has(pair.where);
    const known = themeName === "light" && KNOWN_LIGHT_FAILURES.has(pair.where);
    if (!passes && gated && !known) gatedFailures += 1;
    rows.push({
      theme: themeName,
      where: pair.where,
      fg: `${pair.fg} ${fg}`,
      bg: `${pair.bg} ${bg}`,
      lc: lc.toFixed(1),
      ratio: ratio.toFixed(2),
      need: `Lc ${limit.lc} / ${limit.ratio}:1`,
      verdict: passes ? "pass" : known ? "known" : gated ? "FAIL" : "note",
    });
  }

  return { rows, gatedFailures };
}

const isMain = process.argv[1] && import.meta.url.endsWith(process.argv[1].replace(/\\/g, "/"));

/** Print the designed OKLCH ramp as hex, so the intent and the value sit together. */
if (process.argv.includes("--plan")) {
  for (const [name, L, C] of PLAN.neutrals) {
    const { hex, clipped } = fromOklch(L, C, 90);
    console.log(`${name.padEnd(15)} L ${L}  C ${C}  H 90    ${hex}${clipped ? "  (clipped!)" : ""}`);
  }
  for (const [name, L, C, H] of PLAN.hued) {
    const { hex, clipped } = fromOklch(L, C, H);
    console.log(`${name.padEnd(15)} L ${L}  C ${C}  H ${H}   ${hex}${clipped ? "  (clipped!)" : ""}`);
  }
  process.exit(0);
}

if (isMain || process.argv.includes("--run")) {
  if (drift.length > 0) {
    console.error("tokens.css and tokens.ts have drifted apart:");
    for (const problem of drift) {
      console.error(`  ${problem}`);
    }
    console.error("\nFix the drift first: a value with two homes has none.");
    process.exit(1);
  }

  let failures = 0;
  for (const [name, theme] of [
    ["light", LIGHT],
    ["dark", DARK],
  ]) {
    const { rows, gatedFailures } = audit(name, theme);
    console.log(`\n=== ${name} ===`);
    for (const row of rows) {
      console.log(
        [
          row.verdict.padEnd(5),
          row.lc.padStart(7),
          row.ratio.padStart(6),
          row.need.padEnd(18),
          row.where.padEnd(42),
          `${row.fg} on ${row.bg}`,
        ].join("  "),
      );
    }
    failures += gatedFailures;

    // The ramp itself: is hue held, do steps space evenly, does vividness fall
    // off at the ends? Printed so a ragged step is visible rather than felt.
    console.log(`\n--- ${name} ramp (OKLCH, in lightness order) ---`);
    const order = ["surface-sunken", "paper", "surface", "rule", "faint", "muted", "ink-2", "ink"];
    let previous = null;
    for (const step of order) {
      const { L, C, H } = oklch(theme[step]);
      const gap = previous === null ? "   " : (L - previous).toFixed(3);
      console.log(
        `${step.padEnd(15)} L ${L.toFixed(3)}  C ${C.toFixed(4)}  H ${String(H.toFixed(1)).padStart(5)}   ${theme[step]}   step ${gap}`,
      );
      previous = L;
    }
  }

  console.log(
    `\n${failures === 0 ? "OK" : `${failures} gated failure(s)`}` +
      `  ·  ${Object.keys(LIGHT).length} colour tokens, both notations in step`,
  );
  process.exit(failures === 0 ? 0 : 1);
}
