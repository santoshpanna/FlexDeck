/**
 * Render every icon file from the `[f]` mark.
 *
 * The mark is defined once, as shapes, in packages/ui/src/brand/mark.ts. This
 * draws it into each format a platform asks for, so no binary in the repo is
 * something nobody can regenerate:
 *
 *   desktop  apps/desktop/src-tauri/icons/   .ico + the PNG set `bundle.icon` lists
 *   portal   apps/portal/public/             favicon.svg, favicon.ico, apple-touch-icon.png
 *   android  android/app/src/main/res/       adaptive icon (vector + monochrome), legacy PNGs
 *
 * Rasterised here with no dependencies: each pixel is supersampled against the
 * shapes and box-filtered down, which is plenty for flat geometry.
 *
 * Run: node scripts/gen-brand.mjs   (or `pnpm brand:icons`)
 */
import { deflateSync, crc32 } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  LEFT_BRACKET,
  LETTER_F,
  MARK_BOX,
  MARK_COLORS,
  RIGHT_BRACKET,
  TILE_RADIUS,
  partPath,
  shapeContains,
} from "../packages/ui/src/brand/mark.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = (...parts) => {
  const path = join(root, ...parts);
  mkdirSync(dirname(path), { recursive: true });
  return path;
};

const hex = (value) => [1, 3, 5].map((i) => parseInt(value.slice(i, i + 2), 16));
const TILE = hex(MARK_COLORS.tile);
const BRACKET = hex(MARK_COLORS.bracket);
const LETTER = hex(MARK_COLORS.letter);

const BRACKETS = [...LEFT_BRACKET, ...RIGHT_BRACKET];

// --- raster ------------------------------------------------------------------

function insideRounded(x, y, x0, y0, w, h, r) {
  if (x < x0 || x > x0 + w || y < y0 || y > y0 + h) return false;
  const cx = x < x0 + r ? x0 + r : x > x0 + w - r ? x0 + w - r : x;
  const cy = y < y0 + r ? y0 + r : y > y0 + h - r ? y0 + h - r : y;
  return (x - cx) ** 2 + (y - cy) ** 2 <= r * r;
}

/**
 * Draw the mark at `size` px.
 *
 * - `inset`: empty margin around the tile, as a fraction of the side (legacy
 *   Android icons leave room for the launcher's shadow).
 * - `radius`: tile corner radius as a fraction of the tile; 0 for full-bleed
 *   squares a platform masks itself (Apple touch icons).
 * - `glyph`: glyph scale. Small sizes enlarge it a little, since a 16px icon
 *   cannot spend a third of its width on margin.
 */
function render(size, { inset = 0, radius = TILE_RADIUS, glyph = 1 } = {}) {
  const ss = 8;
  const out = Buffer.alloc(size * size * 4);
  const t0 = size * inset;
  const tw = size - 2 * t0;

  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      let r = 0, g = 0, b = 0, a = 0;
      for (let sy = 0; sy < ss; sy++) {
        for (let sx = 0; sx < ss; sx++) {
          const px = x + (sx + 0.5) / ss;
          const py = y + (sy + 0.5) / ss;
          if (!insideRounded(px, py, t0, t0, tw, tw, tw * radius)) continue;
          // Into mark units, then undo the glyph scale about the centre.
          const c = MARK_BOX / 2;
          const u = c + (((px - t0) / tw) * MARK_BOX - c) / glyph;
          const v = c + (((py - t0) / tw) * MARK_BOX - c) / glyph;
          const color = LETTER_F.some((s) => shapeContains(s, u, v))
            ? LETTER
            : BRACKETS.some((s) => shapeContains(s, u, v))
              ? BRACKET
              : TILE;
          r += color[0];
          g += color[1];
          b += color[2];
          a += 1;
        }
      }
      const i = (y * size + x) * 4;
      if (a > 0) {
        out[i] = Math.round(r / a);
        out[i + 1] = Math.round(g / a);
        out[i + 2] = Math.round(b / a);
        out[i + 3] = Math.round((a / (ss * ss)) * 255);
      }
    }
  }
  return out;
}

/** Optical size: below 48px the glyph grows into some of the tile's margin. */
const glyphFor = (size) => (size <= 16 ? 1.3 : size <= 24 ? 1.2 : size <= 48 ? 1.1 : 1);

// --- encoders ----------------------------------------------------------------

function pngChunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length, 0);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body) >>> 0, 0);
  return Buffer.concat([length, body, crc]);
}

function png(rgba, size) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr.writeUInt8(8, 8); // bit depth
  ihdr.writeUInt8(6, 9); // RGBA
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) rgba.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    pngChunk("IHDR", ihdr),
    pngChunk("IDAT", deflateSync(raw, { level: 9 })),
    pngChunk("IEND", Buffer.alloc(0)),
  ]);
}

/** An .ico whose entries are PNGs (supported since Vista, and by every browser). */
function ico(sizes) {
  const images = sizes.map((size) => ({ size, data: png(render(size, { glyph: glyphFor(size) }), size) }));
  const dir = Buffer.alloc(6 + 16 * images.length);
  dir.writeUInt16LE(1, 2); // 1 = icon
  dir.writeUInt16LE(images.length, 4);
  let offset = dir.length;
  images.forEach(({ size, data }, index) => {
    const at = 6 + index * 16;
    dir.writeUInt8(size >= 256 ? 0 : size, at);
    dir.writeUInt8(size >= 256 ? 0 : size, at + 1);
    dir.writeUInt16LE(1, at + 4);
    dir.writeUInt16LE(32, at + 6);
    dir.writeUInt32LE(data.length, at + 8);
    dir.writeUInt32LE(offset, at + 12);
    offset += data.length;
  });
  return Buffer.concat([dir, ...images.map((image) => image.data)]);
}

const writePng = (path, size, options) => writeFileSync(out(...path), png(render(size, { glyph: glyphFor(size), ...options }), size));

// --- vector ------------------------------------------------------------------

const BRACKET_PATH = partPath(BRACKETS);
const LETTER_PATH = partPath(LETTER_F);

const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${MARK_BOX} ${MARK_BOX}">
  <rect width="${MARK_BOX}" height="${MARK_BOX}" rx="${MARK_BOX * TILE_RADIUS}" fill="${MARK_COLORS.tile}"/>
  <path fill="${MARK_COLORS.bracket}" d="${BRACKET_PATH}"/>
  <path fill="${MARK_COLORS.letter}" d="${LETTER_PATH}"/>
</svg>
`;

/**
 * An adaptive-icon layer: 108dp, with the glyph scaled into the 66dp circle the
 * launcher never masks. At 1.2 the glyph's corners sit 31.5dp from the centre.
 */
function vectorLayer(bracketColor, bracketAlpha, letterColor) {
  const scale = 1.2;
  const shift = 54 - (MARK_BOX / 2) * scale;
  return `<?xml version="1.0" encoding="utf-8"?>
<!-- Generated by scripts/gen-brand.mjs from packages/ui/src/brand/mark.ts. Do not edit. -->
<vector xmlns:android="http://schemas.android.com/apk/res/android"
  android:width="108dp"
  android:height="108dp"
  android:viewportWidth="108"
  android:viewportHeight="108">
  <group android:scaleX="${scale}" android:scaleY="${scale}" android:translateX="${shift}" android:translateY="${shift}">
    <path android:fillColor="${bracketColor}" android:fillAlpha="${bracketAlpha}" android:pathData="${BRACKET_PATH}" />
    <path android:fillColor="${letterColor}" android:pathData="${LETTER_PATH}" />
  </group>
</vector>
`;
}

const adaptive = `<?xml version="1.0" encoding="utf-8"?>
<!-- Generated by scripts/gen-brand.mjs. Do not edit. -->
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
  <background android:drawable="@color/mark_tile" />
  <foreground android:drawable="@drawable/ic_launcher_foreground" />
  <monochrome android:drawable="@drawable/ic_launcher_monochrome" />
</adaptive-icon>
`;

const brandColors = `<?xml version="1.0" encoding="utf-8"?>
<!-- Generated by scripts/gen-brand.mjs from packages/ui/src/brand/mark.ts. Do not edit. -->
<resources>
  <color name="mark_tile">${MARK_COLORS.tile.toUpperCase()}</color>
</resources>
`;

// --- write -------------------------------------------------------------------

const written = [];
const note = (path) => written.push(path);

// Desktop (Tauri): the .ico Windows needs, and the PNGs `bundle.icon` lists.
const tauri = ["apps", "desktop", "src-tauri", "icons"];
writeFileSync(out(...tauri, "icon.ico"), ico([16, 24, 32, 48, 64, 256]));
note("desktop icon.ico");
for (const [file, size] of [["32x32.png", 32], ["128x128.png", 128], ["128x128@2x.png", 256], ["icon.png", 512]]) {
  writePng([...tauri, file], size);
  note(`desktop ${file}`);
}

// Portal: vector favicon for modern browsers, .ico fallback, Apple touch icon
// (full-bleed: iOS rounds it itself).
const pub = ["apps", "portal", "public"];
writeFileSync(out(...pub, "favicon.svg"), svg);
writeFileSync(out(...pub, "favicon.ico"), ico([16, 32, 48]));
writePng([...pub, "apple-touch-icon.png"], 180, { radius: 0 });
note("portal favicon.svg, favicon.ico, apple-touch-icon.png");

// Android: adaptive icon (API 26+, monochrome for themed icons on 33+), and
// legacy PNGs for older launchers.
const res = ["android", "app", "src", "main", "res"];
writeFileSync(out(...res, "drawable", "ic_launcher_foreground.xml"), vectorLayer(MARK_COLORS.bracket, 1, MARK_COLORS.letter));
writeFileSync(out(...res, "drawable", "ic_launcher_monochrome.xml"), vectorLayer("#FFFFFF", 0.5, "#FFFFFF"));
writeFileSync(out(...res, "mipmap-anydpi-v26", "ic_launcher.xml"), adaptive);
writeFileSync(out(...res, "values", "brand.xml"), brandColors);
for (const [dpi, size] of [["mdpi", 48], ["hdpi", 72], ["xhdpi", 96], ["xxhdpi", 144], ["xxxhdpi", 192]]) {
  writePng([...res, `mipmap-${dpi}`, "ic_launcher.png"], size, { inset: 1 / 24 });
}
note("android adaptive icon + 5 legacy densities");

console.log(`wrote:\n  ${written.join("\n  ")}`);
