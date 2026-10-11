/**
 * The ASCII engine: signed distance fields, ray-marched per character cell and
 * shaded into a character ramp.
 *
 * A frame is TWO strings of identical shape — `base` (ink) and `glow` (the
 * orange layer, drawn on top with a bloom). A cell is in at most one of them,
 * so the two `<pre>`s overlay without doubling a character.
 *
 * Pure: no DOM, no clock. Everything time-dependent comes in through
 * `SceneOptions.t`, so a frame is a function of its inputs and can be tested.
 */

export interface Frame {
  base: string;
  glow: string;
}

export type SceneKind = "deck" | "tablet";

export interface SceneOptions {
  kind?: SceneKind;
  cols: number;
  rows: number;
  /** Object rotation about the vertical axis, radians. */
  yaw: number;
  /** Object tilt toward the camera, radians. */
  pitch?: number;
  /** Camera elevation above the floor plane, radians. */
  elev?: number;
  /** Camera distance from the object's centre. */
  dist?: number;
  /** Focal length; larger is tighter. */
  fl?: number;
  /** Where the camera looks, as a height above the centre. */
  lookY?: number;
  /** 0 = assembled, 1 = keycaps lifted off and the body dropped. */
  explode?: number;
  /** Per-key glow, 0..1, indexed row-major from the far-left key. */
  lit?: readonly number[];
  /** 0..1: cells not yet "decoded" show noise; 1 = fully resolved. */
  reveal?: number;
  /** Seconds, for twinkle and decode noise. */
  t?: number;
  /** Draw the floor grid and shadow. */
  floor?: boolean;
}

/** A character cell is ~0.6 as wide as it is tall in the bundled mono face. */
export const CELL_ASPECT = 0.6;

const RAMP = " .,-~:;=!*#$@";
const RL = RAMP.length - 1;
const BAYER = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
const JUNK = "01<>/\\|#%&$@*+=";

export function hash(n: number): number {
  const h = Math.sin(n * 127.1 + 311.7) * 43758.5453;
  return h - Math.floor(h);
}

/** Rounded box distance. Inlined arithmetic, no allocation: this runs millions of times a second. */
function roundBox(x: number, y: number, z: number, bx: number, by: number, bz: number, r: number): number {
  const qx = Math.abs(x) - bx + r;
  const qy = Math.abs(y) - by + r;
  const qz = Math.abs(z) - bz + r;
  const mx = qx > 0 ? qx : 0;
  const my = qy > 0 ? qy : 0;
  const mz = qz > 0 ? qz : 0;
  const m = qx > qy ? (qx > qz ? qx : qz) : qy > qz ? qy : qz;
  return Math.sqrt(mx * mx + my * my + mz * mz) + (m < 0 ? m : 0) - r;
}

interface Camera {
  rox: number;
  roy: number;
  roz: number;
  fx: number;
  fy: number;
  fz: number;
  rx: number;
  rz: number;
  ux: number;
  uy: number;
  uz: number;
  fl: number;
  asp: number;
}

function camera(o: SceneOptions): Camera {
  const dist = o.dist ?? 5.2;
  const el = o.elev ?? 0.8;
  const rox = 0;
  const roy = dist * Math.sin(el);
  const roz = -dist * Math.cos(el);
  let fx = -rox;
  let fy = -roy + (o.lookY ?? 0);
  let fz = -roz;
  const fn = Math.hypot(fx, fy, fz);
  fx /= fn;
  fy /= fn;
  fz /= fn;
  let rx = fz;
  let rz = -fx;
  const rn = Math.hypot(rx, rz);
  rx /= rn;
  rz /= rn;
  // up = forward × right (right has no y component)
  const ux = fy * rz;
  const uy = fz * rx - fx * rz;
  const uz = -fy * rx;
  return { rox, roy, roz, fx, fy, fz, rx, rz, ux, uy, uz, fl: o.fl ?? 2.5, asp: (o.cols * CELL_ASPECT) / o.rows };
}

/** Number of individually lit keys a scene has. */
export function keyCount(kind: SceneKind): number {
  return kind === "deck" ? 15 : 35;
}

/** Render one frame of a scene. */
export function render(o: SceneOptions): Frame {
  const C = o.cols;
  const R = o.rows;
  const kind = o.kind ?? "deck";
  const ex = o.explode ?? 0;
  const t = o.t ?? 0;
  const lit = o.lit ?? [];
  const reveal = o.reveal ?? 1;
  const floor = o.floor ?? true;
  const cy = Math.cos(o.yaw);
  const sy = Math.sin(o.yaw);
  const cp = Math.cos(o.pitch ?? 0);
  const sp = Math.sin(o.pitch ?? 0);
  const cam = camera(o);

  let lx = -0.42;
  let ly = 0.82;
  let lz = -0.4;
  const ln = Math.hypot(lx, ly, lz);
  lx /= ln;
  ly /= ln;
  lz /= ln;

  const keys = keyCount(kind);
  const lifts = new Float64Array(keys);
  for (let k = 0; k < keys; k++) lifts[k] = ex * (0.45 + 1.25 * hash(k + 3));

  let hitId = -1;
  const map =
    kind === "deck"
      ? (x: number, y: number, z: number): number => {
          let d = roundBox(x, y + ex * 0.35, z, 1.62, 0.16, 1.02, 0.14);
          // The recess the keys sit in.
          const recess = roundBox(x, y - 0.17 + ex * 0.35, z, 1.48, 0.03, 0.88, 0.05);
          d = d > -recess ? d : -recess;
          const s = 0.56;
          let ix = Math.round(x / s);
          ix = ix < -2 ? -2 : ix > 2 ? 2 : ix;
          let iz = Math.round(z / s);
          iz = iz < -1 ? -1 : iz > 1 ? 1 : iz;
          const k = (iz + 1) * 5 + (ix + 2);
          const dk = roundBox(x - ix * s, y - 0.2 - lifts[k]!, z - iz * s, 0.215, 0.085, 0.215, 0.07);
          hitId = -1;
          if (dk < d) {
            d = dk;
            hitId = k;
          }
          return d;
        }
      : (x: number, y: number, z: number): number => {
          let d = roundBox(x, y + ex * 0.25, z, 1.55, 0.06, 1.02, 0.14);
          const s = 0.4;
          let ix = Math.round(x / s);
          ix = ix < -3 ? -3 : ix > 3 ? 3 : ix;
          let iz = Math.round(z / s);
          iz = iz < -2 ? -2 : iz > 2 ? 2 : iz;
          const k = (iz + 2) * 7 + (ix + 3);
          const dk = roundBox(x - ix * s, y - 0.075 - lifts[k]! * 0.6, z - iz * s, 0.16, 0.022, 0.16, 0.05);
          hitId = -1;
          if (dk < d) {
            d = dk;
            hitId = k;
          }
          return d;
        };

  // A ray that misses this sphere cannot hit anything: skip the march.
  const bound = (kind === "deck" ? 2.05 : 1.95) + ex * 1.7;
  const b2 = bound * bound;

  // The camera, carried into object space once.
  const oz0 = cam.rox * sy + cam.roz * cy;
  const ox = cam.rox * cy - cam.roz * sy;
  const oy = cam.roy * cp + oz0 * sp;
  const oz = -cam.roy * sp + oz0 * cp;

  const toObject = (x: number, y: number, z: number): [number, number, number] => {
    const tz0 = x * sy + z * cy;
    return [x * cy - z * sy, y * cp + tz0 * sp, -y * sp + tz0 * cp];
  };

  let base = "";
  let glow = "";
  for (let j = 0; j < R; j++) {
    let lb = "";
    let lg = "";
    const v = 1 - ((j + 0.5) / R) * 2;
    for (let i = 0; i < C; i++) {
      const u = (((i + 0.5) / C) * 2 - 1) * cam.asp;
      let dx = cam.fx * cam.fl + cam.rx * u + cam.ux * v;
      let dy = cam.fy * cam.fl + cam.uy * v;
      let dz = cam.fz * cam.fl + cam.rz * u + cam.uz * v;
      const dn = Math.hypot(dx, dy, dz);
      dx /= dn;
      dy /= dn;
      dz /= dn;
      const ez0 = dx * sy + dz * cy;
      const ex1 = dx * cy - dz * sy;
      const ey = dy * cp + ez0 * sp;
      const ez = -dy * sp + ez0 * cp;

      let ch = " ";
      let gch = " ";
      const bb = ox * ex1 + oy * ey + oz * ez;
      const cc = ox * ox + oy * oy + oz * oz - b2;
      const h = bb * bb - cc;
      let hit = false;
      let px = 0;
      let py = 0;
      let pz = 0;
      if (h > 0) {
        const sh = Math.sqrt(h);
        let tt = Math.max(0, -bb - sh);
        const tmax = -bb + sh;
        for (let step = 0; step < 64; step++) {
          px = ox + ex1 * tt;
          py = oy + ey * tt;
          pz = oz + ez * tt;
          const d = map(px, py, pz);
          if (d < 0.003) {
            hit = true;
            break;
          }
          tt += d * 0.85;
          if (tt > tmax) break;
        }
      }
      const dither = (BAYER[(j & 3) * 4 + (i & 3)]! / 16 - 0.5) / RL;
      if (hit) {
        map(px, py, pz);
        const id = hitId;
        const e = 0.003;
        let nx = map(px + e, py, pz) - map(px - e, py, pz);
        let ny = map(px, py + e, pz) - map(px, py - e, pz);
        let nz = map(px, py, pz + e) - map(px, py, pz - e);
        const nn = Math.hypot(nx, ny, nz) || 1;
        nx /= nn;
        ny /= nn;
        nz /= nn;
        // Normal back to world space, so the light stays put while the object turns.
        const ny2 = ny * cp - nz * sp;
        const nz2 = ny * sp + nz * cp;
        const wx = nx * cy + nz2 * sy;
        const wz = -nx * sy + nz2 * cy;
        const diff = Math.max(0, wx * lx + ny2 * ly + wz * lz);
        let rim = 1 + (wx * dx + ny2 * dy + wz * dz);
        rim = rim * rim * rim * 0.45;
        // Materials, so the parts read apart at a glance: bright keycaps, a
        // darker body, and a recess darker still.
        const inRecess = id < 0 && py > 0.1 - ex * 0.35 && Math.abs(px) < 1.5 && Math.abs(pz) < 0.9;
        const albedo = id >= 0 ? 1 : inRecess ? 0.32 : 0.62;
        const lum = Math.min(1, 0.05 + albedo * (0.82 * diff + rim));
        let ci = Math.floor((lum + dither) * RL + 0.5);
        ci = ci < 1 ? 1 : ci > RL ? RL : ci;
        if (id >= 0 && (lit[id] ?? 0) > 0.35) gch = RAMP.charAt(Math.max(7, ci));
        else ch = RAMP.charAt(ci);
      } else if (floor && dy < 0) {
        const tf = (-0.62 - cam.roy) / dy;
        const gx = cam.rox + dx * tf;
        const gz = cam.roz + dz * tf;
        const fade = 1 - tf / 16;
        if (fade > 0) {
          const a = Math.abs(gx * 1.25 - Math.round(gx * 1.25));
          const bz = Math.abs(gz * 1.25 - Math.round(gz * 1.25));
          let shadow = false;
          for (let q = 1; q <= 3 && !shadow; q++) {
            const [sx, sy2, sz] = toObject(gx + lx * q * 0.45, -0.62 + ly * q * 0.45, gz + lz * q * 0.45);
            if (map(sx, sy2, sz) < 0.12) shadow = true;
          }
          if (!shadow) {
            if (a < 0.045 && bz < 0.045) ch = fade > 0.45 ? "+" : ".";
            else if (Math.min(a, bz) < 0.018 && fade > 0.2 && (i + j) % 2 === 0) ch = ".";
          }
        }
      } else {
        const star = hash(i * 3.17 + j * 91.3);
        if (star > 0.992) ch = Math.sin(t * 2 + star * 80) > 0.6 ? "*" : ".";
      }
      if (reveal < 1) {
        const r = hash(i * 1.37 + j * 7.91);
        if (r > reveal && (ch !== " " || gch !== " " || r - reveal < 0.08)) {
          ch = JUNK.charAt(Math.floor(hash(i + j * 31 + Math.floor(t * 20)) * JUNK.length));
          gch = " ";
        }
      }
      lb += gch !== " " ? " " : ch;
      lg += gch;
    }
    base += lb + "\n";
    glow += lg + "\n";
  }
  return { base, glow };
}

/**
 * Where a point in object space lands on the character grid, as fractions of
 * the frame (0..1 across, 0..1 down). For pinning HTML callouts to the model.
 */
export function project(o: SceneOptions, x: number, y: number, z: number): { x: number; y: number } {
  const cy = Math.cos(o.yaw);
  const sy = Math.sin(o.yaw);
  const cp = Math.cos(o.pitch ?? 0);
  const sp = Math.sin(o.pitch ?? 0);
  const cam = camera(o);
  const y2 = y * cp - z * sp;
  const z2 = y * sp + z * cp;
  const wx = x * cy + z2 * sy;
  const wz = -x * sy + z2 * cy;
  const qx = wx - cam.rox;
  const qy = y2 - cam.roy;
  const qz = wz - cam.roz;
  const xc = qx * cam.rx + qz * cam.rz;
  const yc = qx * cam.ux + qy * cam.uy + qz * cam.uz;
  const zc = qx * cam.fx + qy * cam.fy + qz * cam.fz;
  const u = (xc * cam.fl) / zc;
  const v = (yc * cam.fl) / zc;
  return { x: (u / cam.asp + 1) / 2, y: (1 - v) / 2 };
}

/**
 * Resolve `text` out of noise: each character settles at its own moment, so a
 * headline decodes rather than fades. Spaces and newlines are kept from the start
 * so the layout never shifts.
 */
export function decode(text: string, t: number): string {
  let out = "";
  for (let i = 0; i < text.length; i++) {
    const c = text.charAt(i);
    if (c === " " || c === "\n") {
      out += c;
      continue;
    }
    const at = 0.25 + hash(i + 9) * 1.1 + i * 0.012;
    out += t >= at ? c : JUNK.charAt(Math.floor(hash(i * 13 + Math.floor(t * 24)) * JUNK.length));
  }
  return out;
}

/** When `decode(text, t)` has fully settled. */
export function decodeDuration(text: string): number {
  let last = 0;
  for (let i = 0; i < text.length; i++) last = Math.max(last, 0.25 + hash(i + 9) * 1.1 + i * 0.012);
  return last;
}
