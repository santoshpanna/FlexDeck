import { CELL_ASPECT, hash, type Frame } from "./engine.ts";

/**
 * Small 2D character fields: the animated figures in cards, empty states and
 * loading states. Each is a pure function of size and time.
 */

export type FieldKind = "rings" | "pair" | "faders" | "flow" | "rail" | "rain" | "install" | "scan" | "globe" | "pulse" | "typing" | "automate";

function grid(cols: number, rows: number, cell: (x: number, y: number) => [string, string]): Frame {
  let base = "";
  let glow = "";
  for (let y = 0; y < rows; y++) {
    let lb = "";
    let lg = "";
    for (let x = 0; x < cols; x++) {
      const [ch, g] = cell(x, y);
      lb += g !== " " ? " " : ch;
      lg += g;
    }
    base += lb + "\n";
    glow += lg + "\n";
  }
  return { base, glow };
}

/** A device on the left broadcasting rings to a tablet on the right: pairing. */
export function rings(t: number, cols = 46, rows = 13): Frame {
  const cx = 6;
  const cy = Math.floor(rows / 2);
  const tx = cols - 8;
  return grid(cols, rows, (x, y) => {
    if (x >= cx - 3 && x <= cx + 3 && y >= cy - 3 && y <= cy + 3) {
      return [x === cx - 3 || x === cx + 3 ? "|" : y === cy - 3 || y === cy + 3 ? "-" : ":", " "];
    }
    if (x >= tx && x <= tx + 6 && y >= cy - 2 && y <= cy + 2) {
      return [y === cy - 2 || y === cy + 2 ? "=" : x === tx || x === tx + 6 ? "|" : "#", " "];
    }
    const dx = (x - cx) * 0.55;
    const dy = y - cy;
    const r = Math.hypot(dx, dy);
    const w = Math.sin(r * 1.1 - t * 5);
    if (w > 0.93 && r < 26) return [" ", r < 10 ? ")" : "}"];
    if (w > 0.6 && r < 26) return [".", " "];
    return [" ", " "];
  });
}

/** A bank of faders riding their levels. */
export function faders(t: number, cols = 46, rows = 13): Frame {
  return grid(cols, rows, (x, y) => {
    const col = Math.floor(x / 6);
    const off = x % 6;
    if (off < 1 || off > 3 || col >= Math.floor(cols / 6)) return [" ", " "];
    const level = Math.floor((0.5 + 0.45 * Math.sin(t * 1.6 + col * 0.8) * Math.cos(t * 0.7 + col)) * (rows - 2)) + 1;
    const up = rows - 1 - y;
    if (up === level) return [" ", "="];
    if (up < level) return [off === 2 ? "|" : ":", " "];
    return [off === 2 ? "." : " ", " "];
  });
}

/** WHEN → IF → DO, with packets running along the wire. */
export function flow(t: number, cols = 46, rows = 13): Frame {
  const top = 2;
  const bottom = rows - 3;
  const bend = Math.floor(cols / 2) - 1;
  const len = bend - 7 + (bottom - top) + (cols - 9 - bend);
  const labels: [string, number, number][] = [
    ["[WHEN]", 1, top],
    ["[IF]", bend - 2, Math.floor((top + bottom) / 2)],
    ["[DO]", cols - 8, bottom],
  ];
  const packets: [number, number, string][] = [];
  for (let k = 0; k < 3; k++) {
    const p = (t * 14 + k * 13) % len;
    const a = bend - 7;
    const b = bottom - top;
    if (p < a) packets.push([7 + Math.floor(p), top, ">"]);
    else if (p < a + b) packets.push([bend, top + Math.floor(p - a), "v"]);
    else packets.push([bend + Math.floor(p - a - b), bottom, ">"]);
  }
  return grid(cols, rows, (x, y) => {
    for (const [px, py, c] of packets) if (px === x && py === y) return [" ", c];
    for (const [text, x0, y0] of labels) if (y === y0 && x >= x0 && x < x0 + text.length) return [text.charAt(x - x0), " "];
    const onWire =
      (y === top && x >= 7 && x <= bend) || (x === bend && y >= top && y <= bottom) || (y === bottom && x >= bend && x < cols - 8);
    if (onWire) return [x === bend && y > top && y < bottom ? "|" : "-", " "];
    return [" ", " "];
  });
}

/** One automation, as it reads in the editor: a trigger, a condition, an action. */
const RUNS: { when: [string, string]; check: string; act: [string, string]; fails?: boolean }[] = [
  { when: ["obs", "went live"], check: "mic on", act: ["hue", "lights red"] },
  { when: ["clock", "09:00 weekday"], check: "at desk", act: ["spotify", "focus mix"] },
  { when: ["discord", "joined voice"], check: "streaming", act: ["obs", "mute desktop"], fails: true },
  { when: ["deck", "key held"], check: "in game", act: ["voicemeeter", "duck music"] },
  { when: ["twitch", "new follow"], check: "live", act: ["hue", "flash x3"] },
];

/**
 * An automation running, round after round: the WHEN box fires and types out
 * its event, a packet runs the wire to the IF gate, the gate thinks — and
 * either passes it on to light the DO box, or says no and the packet comes
 * apart. Laid out from `cols`, so it fills whatever width it is given.
 */
export function automate(t: number, cols = 46, rows = 13): Frame {
  const period = 5.2;
  const round = Math.floor(t / period);
  const s = t - round * period;
  const run = RUNS[((round % RUNS.length) + RUNS.length) % RUNS.length]!;
  const passes = !run.fails;

  const mid = Math.floor(rows / 2);
  const m = Math.max(5, Math.floor(cols * 0.07));
  const bw = Math.min(24, Math.max(14, Math.floor((cols - 2 * m - 14) * 0.36)));
  const w0 = m;
  const w1 = m + bw - 1;
  const d1 = cols - 1 - m;
  const d0 = d1 - bw + 1;
  const g0 = Math.floor((w1 + d0) / 2) - 4;
  const g1 = g0 + 7;

  // The beats of a round, in seconds.
  const FIRE = 0.9;
  const LEG_A = 1.7;
  const THINK = 2.4;
  const LEG_B = 3.5;
  const DONE = 4.7;

  const firing = s < FIRE;
  const thinking = s >= LEG_A && s < THINK;
  const decided = s >= THINK;
  const acting = passes && s >= LEG_B;
  const doLit = acting && s < DONE;

  // The packet: a head on one leg of the wire, with a fading trail behind it.
  let head = -1;
  if (s >= FIRE && s < LEG_A) head = w1 + 1 + Math.floor(((s - FIRE) / (LEG_A - FIRE)) * (g0 - w1 - 1));
  if (passes && s >= THINK + 0.4 && s < LEG_B) head = g1 + 1 + Math.floor(((s - THINK - 0.4) / (LEG_B - THINK - 0.4)) * (d0 - g1 - 1));

  const typed = (text: string, from: number) => text.slice(0, Math.max(0, Math.floor((s - from) * 26)));
  const fit = (text: string, room: number) => (text.length > room ? text.slice(0, room - 1) + "~" : text);
  const room = bw - 4;

  const box = (x: number, y: number, x0: number, x1: number, title: string, plugin: string, text: string, lit: boolean, cursor: boolean): [string, string] | null => {
    if (x < x0 || x > x1 || y < mid - 2 || y > mid + 2) return null;
    const edge = (c: string): [string, string] => (lit ? [" ", c] : [c, " "]);
    const top = y === mid - 2;
    if ((top || y === mid + 2) && (x === x0 || x === x1)) return edge("+");
    if (top) {
      const label = ` ${title} `;
      const i = x - (x0 + 2);
      if (i >= 0 && i < label.length) return edge(label.charAt(i));
      return edge("-");
    }
    if (y === mid + 2) return edge("-");
    if (x === x0 || x === x1) return edge("|");
    const i = x - (x0 + 2);
    if (y === mid - 1 && i >= 0 && i < plugin.length) return [plugin.charAt(i), " "];
    if (y === mid) {
      if (i >= 0 && i < text.length) return lit ? [" ", text.charAt(i)] : [text.charAt(i), " "];
      if (cursor && i === text.length && Math.floor(t * 6) % 2 === 0) return [" ", "_"];
    }
    return [" ", " "];
  };

  const whenText = typed(fit(run.when[1], room), 0.05);
  // Before it runs, the DO box waits; on a refused run it says it was skipped.
  const doText = acting
    ? typed(fit(run.act[1], room), LEG_B)
    : !passes && decided
      ? typed("skipped", THINK + 0.3)
      : ".".repeat(1 + (Math.floor(t * 3) % 3));
  const spinner = "|/-\\".charAt(Math.floor(t * 12) % 4);
  const verdict = decided ? (passes ? "ok" : "no") : thinking ? ` ${spinner}` : " ?";
  const check = fit(run.check, g1 - g0 + 6);
  const checkX = Math.floor((g0 + g1) / 2 - check.length / 2);

  // Where the DO box sends its ripple, and where a refused packet comes apart.
  const dcx = (d0 + d1) / 2;
  const gcx = (g0 + g1) / 2;

  return grid(cols, rows, (x, y) => {
    const when = box(x, y, w0, w1, "WHEN", fit(run.when[0], room), whenText, firing, firing);
    if (when) return when;
    const act = box(x, y, d0, d1, "DO", acting ? fit(run.act[0], room) : "", doText, doLit, doLit && doText.length < fit(run.act[1], room).length);
    if (act) return act;

    // The gate: a diamond-ish lozenge with its verdict inside.
    if (x >= g0 && x <= g1 && y >= mid - 1 && y <= mid + 1) {
      const lit = decided || thinking;
      const glowIf = (c: string): [string, string] => (lit ? [" ", c] : [c, " "]);
      if (y === mid - 1) return x === g0 ? glowIf("/") : x === g1 ? glowIf("\\") : glowIf("-");
      if (y === mid + 1) return x === g0 ? glowIf("\\") : x === g1 ? glowIf("/") : glowIf("-");
      if (x === g0) return glowIf("<");
      if (x === g1) return glowIf(">");
      const i = x - g0;
      if (i === 1 || i === 2) return ["IF".charAt(i - 1), " "];
      if (i === 4 || i === 5) return decided || thinking ? [" ", verdict.charAt(i - 4)] : [verdict.charAt(i - 4), " "];
      return [" ", " "];
    }
    if (y === mid + 2 && x >= checkX && x < checkX + check.length) return [check.charAt(x - checkX), " "];

    // A refused packet bursts at the gate.
    if (!passes && s >= THINK && s < THINK + 0.9) {
      const r = Math.hypot((x - gcx) * 0.5, y - mid);
      const front = 2.2 + (s - THINK) * 5;
      if (Math.abs(r - front) < 0.4 && hash(x * 7.3 + y * 3.1 + round) > 0.55) return [" ", s - THINK < 0.4 ? "*" : "."];
    }

    // The DO box answering: rings spreading out from it.
    // Outlines of the DO box, one step further out each beat, fading to dots
    // as they go: a step is two columns across and one row down, so they keep
    // the box's shape.
    if (acting && s < DONE + 0.6) {
      const ex = x > d1 ? x - d1 : x < d0 ? d0 - x : 0;
      const ey = Math.max(0, Math.abs(y - mid) - 2);
      const age = s - LEG_B;
      // As many as fit in the margin: a ring cut off by the frame reads as a glitch.
      for (let n = 1; n <= Math.min(3, Math.floor(m / 2)); n++) {
        const born = (n - 1) * 0.18;
        const life = age - born;
        if (life < 0 || life > 1.1) continue;
        const side = ey < n && ex === 2 * n;
        const edge = ey === n && ex <= 2 * n && (ex === 2 * n || ex % 2 === 0 || ex === 0);
        if (!side && !edge) continue;
        const ch = edge ? (ex === 2 * n ? ((x > dcx) === (y < mid) ? "\\" : "/") : "-") : x > dcx ? ")" : "(";
        return life < 0.5 ? [" ", ch] : [life < 0.85 ? ch : ".", " "];
      }
    }

    // The wire, and the packet running it.
    const onWire = y === mid && ((x > w1 && x < g0) || (x > g1 && x < d0));
    if (onWire) {
      if (head >= 0) {
        const d = head - x;
        if (d === 0) return [" ", ">"];
        if (d > 0 && d <= 2) return [" ", "="];
        if (d > 2 && d <= 5) return ["=", " "];
      }
      return [x % 2 === 0 ? "-" : " ", " "];
    }
    return [" ", " "];
  });
}

/** Plugin names falling like rain, their heads lit. */
export function rain(t: number, cols = 46, rows = 13): Frame {
  const words = "obsspotifydiscordhuetwitchvoicemeeterkeyboardmidishell";
  return grid(cols, rows, (x, y) => {
    if (x % 2 !== 0) return [" ", " "];
    const speed = 3 + ((x * 7) % 5);
    const head = Math.floor((t * speed + x * 3.7) % (rows + 9));
    const d = head - y;
    if (d < 0 || d >= 7) return [" ", " "];
    const c = words.charAt((x * 5 + y + Math.floor(t * 3)) % words.length);
    if (d === 0) return [" ", c.toUpperCase()];
    return [d < 3 ? c : ".", " "];
  });
}

/** A sweep line over static: waiting, searching. */
export function scan(t: number, cols = 46, rows = 13): Frame {
  const line = Math.floor(((t * 0.6) % 1) * (cols + 10)) - 5;
  return grid(cols, rows, (x, y) => {
    const d = x - line;
    if (d >= -1 && d <= 0) return [" ", y % 2 === 0 ? "|" : ":"];
    const n = hash(x * 3.1 + y * 17.7 + Math.floor(t * 4));
    if (d < 0 && d > -8) return [n > 0.6 ? "." : " ", " "];
    return [n > 0.93 ? "." : " ", " "];
  });
}

/**
 * A globe turning, its cities lighting as they come round, with a signal
 * circling it: the web, reaching this computer from anywhere.
 */
export function globe(t: number, cols = 46, rows = 13): Frame {
  const cx = Math.floor(cols / 2);
  const cy = Math.floor(rows / 2);
  const r = Math.floor(rows / 2) - 0.5;
  const spin = t * 0.5;
  const step = Math.PI / 6;
  const cities: [number, number][] = [
    [0.3, 0.6],
    [1.9, -0.4],
    [3.4, 0.2],
    [4.6, 0.75],
    [5.5, -0.7],
  ];
  // The orbit: a tilted ellipse wider than the globe, with a signal running round it.
  const ox = (r + 2.6) / 0.55;
  const oy = r * 0.5;
  const signal = t * 1.4;

  return grid(cols, rows, (x, y) => {
    const u = ((x - cx) * 0.55) / r;
    const v = (y - cy) / r;
    const d = u * u + v * v;

    if (d <= 1) {
      const z = Math.sqrt(1 - d);
      const lat = Math.asin(v);
      const lon = Math.atan2(u, z) + spin;
      for (const [cl, ca] of cities) {
        const dl = Math.abs(((lon - cl + Math.PI * 3) % (Math.PI * 2)) - Math.PI);
        if (dl < 0.16 && Math.abs(lat - ca) < 0.2 && z > 0.25) return [" ", "o"];
      }
      const light = 0.35 + 0.65 * (-u * 0.5 - v * 0.3 + z * 0.8);
      const nearLon = Math.abs(((lon % step) + step) % step - step / 2) > step / 2 - 0.09 * (1 + (1 - z) * 2);
      const nearLat = Math.abs(((lat % step) + step) % step - step / 2) > step / 2 - 0.12;
      if (d > 0.8) return [light > 0.4 ? ":" : ".", " "];
      if (nearLon && nearLat) return ["+", " "];
      if (nearLon) return [light > 0.5 ? ":" : ".", " "];
      if (nearLat) return [light > 0.5 ? "-" : ".", " "];
      return [light > 0.75 ? "." : " ", " "];
    }

    // On the orbit, in front of the globe or clear of it.
    const a = Math.atan2((y - cy) / oy, (x - cx) / ox);
    const e = Math.hypot((x - cx) / ox, (y - cy) / oy);
    if (Math.abs(e - 1) < 0.5 / oy) {
      // Signed: how far behind the signal's head this point is.
      const behind = ((signal - a) % (Math.PI * 2) + Math.PI * 2) % (Math.PI * 2);
      if (behind < 0.07 || behind > Math.PI * 2 - 0.02) return [" ", "*"];
      if (behind < 0.45) return [" ", "."];
      return [Math.floor(x) % 2 === 0 ? "." : " ", " "];
    }
    return [" ", " "];
  });
}

/**
 * A tablet face-on, its keys lighting in a ripple that starts from a different
 * key each time, and faint rings spreading out from it: a deck coming alive.
 */
export function pulse(t: number, cols = 46, rows = 13): Frame {
  const kw = 4; // key width, in cells
  const kh = 2;
  const across = 5;
  const down = 3;
  const w = across * (kw + 1) + 3; // keys, gaps, and the bezel either side
  const h = down * (kh + 1) + 1;
  const x0 = Math.floor((cols - w) / 2);
  const y0 = Math.floor((rows - h) / 2);
  const cx = x0 + w / 2;
  const cy = y0 + h / 2;

  // Each round the ripple starts from another key.
  const period = 2.2;
  const round = Math.floor(t / period);
  const age = t - round * period;
  const origin = Math.floor(hash(round * 13.1) * across * down);
  const ox = origin % across;
  const oy = Math.floor(origin / across);

  return grid(cols, rows, (x, y) => {
    const inside = x >= x0 && x < x0 + w && y >= y0 && y < y0 + h;
    if (inside) {
      // The bezel: dotted, brighter at the corners.
      const edgeX = x === x0 || x === x0 + w - 1;
      const edgeY = y === y0 || y === y0 + h - 1;
      if (edgeX && edgeY) return ["+", " "];
      if (edgeY) return [x % 2 === 0 ? "-" : " ", " "];
      if (edgeX) return [":", " "];

      const lx = x - x0 - 2;
      const ly = y - y0 - 1;
      const kx = Math.floor(lx / (kw + 1));
      const ky = Math.floor(ly / (kh + 1));
      const onKey = lx >= 0 && kx < across && lx % (kw + 1) < kw && ky < down && ly % (kh + 1) < kh;
      if (!onKey) return [" ", " "];

      // How lit the key is: a ring of light moving out from the origin key.
      const dist = Math.hypot(kx - ox, (ky - oy) * 1.4);
      const front = age * 3.2;
      const lit = Math.max(0, 1 - Math.abs(dist - front) / 0.9) * Math.max(0, 1 - age / period);
      if (lit > 0.55) return [" ", "#"];
      if (lit > 0.2) return ["=", " "];
      return [":", " "];
    }

    // Faint rings spreading out from the tablet.
    const dx = (x - cx) * 0.5;
    const dy = y - cy;
    const r = Math.hypot(dx, dy);
    const wave = Math.sin(r * 1.3 - t * 4);
    if (wave > 0.95 && r > h / 2 + 1 && r < 26) return [r < 15 ? "." : " ", " "];
    return [" ", " "];
  });
}

/** A code being typed into a box, key by key, then accepted: waiting for a browser. */
export function typing(t: number, cols = 46, rows = 3): Frame {
  const cycle = 4.5;
  const round = Math.floor(t / cycle);
  const phase = (t % cycle) / cycle;
  const alphabet = "ABCDEFGHJKMNPQRSTVWXYZ23456789";
  const code = Array.from({ length: 9 }, (_, i) =>
    i === 4 ? "-" : alphabet.charAt(Math.floor(hash(round * 31.7 + i * 7.3) * alphabet.length)),
  ).join("");
  const typed = Math.min(code.length, Math.floor(phase * 16));
  const done = typed === code.length;
  const blink = Math.floor(t * 3) % 2 === 0;

  const width = code.length + 6;
  const x0 = Math.floor((cols - width) / 2);
  const x1 = x0 + width - 1;
  const top = Math.floor(rows / 2) - 1;
  const mid = top + 1;

  return grid(cols, rows, (x, y) => {
    if (x >= x0 && x <= x1 && y >= top && y <= top + 2) {
      if (y !== mid) return ["-", " "];
      if (x === x0 || x === x1) return ["|", " "];
      if (x === x0 + 2) return [" ", ">"];
      const i = x - (x0 + 3);
      if (i >= 0 && i < typed) return done || i === typed - 1 ? [" ", code.charAt(i)] : [code.charAt(i), " "];
      if (i === typed && !done) return blink ? [" ", "_"] : [" ", " "];
      return [" ", " "];
    }
    // Sparse static either side, as the tablet's scan has.
    const n = hash(x * 3.1 + y * 17.7 + Math.floor(t * 4));
    return [n > 0.94 ? "." : " ", " "];
  });
}

// -- small solids: ray-marched, shaded into the character ramp ---------------

type Vec = [number, number, number];

const SHADE = " .,-~:;=!*#$@";

const len2 = (x: number, y: number) => Math.sqrt(x * x + y * y);
const len3 = (x: number, y: number, z: number) => Math.sqrt(x * x + y * y + z * z);
const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
const easeOut = (p: number) => 1 - Math.pow(1 - clamp01(p), 3);
const easeIn = (p: number) => Math.pow(clamp01(p), 2);

function roundBox(x: number, y: number, z: number, bx: number, by: number, bz: number, r: number): number {
  const qx = Math.abs(x) - bx;
  const qy = Math.abs(y) - by;
  const qz = Math.abs(z) - bz;
  return len3(Math.max(qx, 0), Math.max(qy, 0), Math.max(qz, 0)) + Math.min(Math.max(qx, qy, qz), 0) - r;
}

function rotate(a: number, b: number, angle: number): [number, number] {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  return [a * c - b * s, a * s + b * c];
}

/** Distance to the scene, and which material is nearest. */
type Scene = (x: number, y: number, z: number) => [number, number];

interface View {
  yaw: number;
  pitch: number;
  /** World units per character column. */
  scale: number;
  target?: Vec;
}

/**
 * An orthographic camera over a solid scene, and a character layer to draw on:
 * the solid first, then marks, dots and words placed in its screen space.
 */
function studio(cols: number, rows: number, view: View) {
  const { yaw, pitch, scale } = view;
  const [tx, ty, tz] = view.target ?? [0, 0, 0];
  const cp = Math.cos(pitch);
  const sp = Math.sin(pitch);
  const cy = Math.cos(yaw);
  const sy = Math.sin(yaw);
  const turn = (x: number, y: number, z: number): Vec => [x * cy + z * sy, y, -x * sy + z * cy];
  const fwd = turn(0, -sp, cp);
  const up = turn(0, cp, sp);
  const right = turn(1, 0, 0);
  const rowScale = scale / CELL_ASPECT;

  const base = Array.from({ length: rows }, () => Array<string>(cols).fill(" "));
  const glow = Array.from({ length: rows }, () => Array<string>(cols).fill(" "));
  const mats = new Int8Array(cols * rows).fill(-1);
  const lums = new Float32Array(cols * rows);

  // A point light up and to the left, near enough that flat faces shade across.
  const lamp: Vec = [tx - 1.7 * right[0] + 1.9 * up[0] - 3.2 * fwd[0], ty - 1.7 * right[1] + 1.9 * up[1] - 3.2 * fwd[1], tz - 1.7 * right[2] + 1.9 * up[2] - 3.2 * fwd[2]];

  /** Ray-march every cell; `lit` says which materials glow, `gain` how bright each is. */
  const shoot = (scene: Scene, lit: (mat: number) => boolean = () => false, gain: (mat: number) => number = () => 1) => {
    for (let r = 0; r < rows; r++) {
      for (let c = 0; c < cols; c++) {
        const u = (c - cols / 2 + 0.5) * scale;
        const v = -(r - rows / 2 + 0.5) * rowScale;
        let ox = tx + right[0] * u + up[0] * v - fwd[0] * 8;
        let oy = ty + right[1] * u + up[1] * v - fwd[1] * 8;
        let oz = tz + right[2] * u + up[2] * v - fwd[2] * 8;
        let travelled = 0;
        let mat = -1;
        for (let i = 0; i < 56 && travelled < 16; i++) {
          const [d, m] = scene(ox, oy, oz);
          if (d < 0.003) {
            mat = m;
            break;
          }
          ox += fwd[0] * d;
          oy += fwd[1] * d;
          oz += fwd[2] * d;
          travelled += d;
        }
        if (mat < 0) continue;
        const e = 0.004;
        const nx = scene(ox + e, oy, oz)[0] - scene(ox - e, oy, oz)[0];
        const ny = scene(ox, oy + e, oz)[0] - scene(ox, oy - e, oz)[0];
        const nz = scene(ox, oy, oz + e)[0] - scene(ox, oy, oz - e)[0];
        const nl = len3(nx, ny, nz) || 1;
        let lx = lamp[0] - ox;
        let ly = lamp[1] - oy;
        let lz = lamp[2] - oz;
        const ll = len3(lx, ly, lz);
        lx /= ll;
        ly /= ll;
        lz /= ll;
        let hx = lx - fwd[0];
        let hy = ly - fwd[1];
        let hz = lz - fwd[2];
        const hl = len3(hx, hy, hz);
        hx /= hl;
        hy /= hl;
        hz /= hl;
        const diffuse = Math.max(0, (nx * lx + ny * ly + nz * lz) / nl);
        const spec = Math.pow(Math.max(0, (nx * hx + ny * hy + nz * hz) / nl), 28);
        // Edges turned away from the eye catch a little rim light.
        const rim = Math.pow(1 - Math.abs((nx * fwd[0] + ny * fwd[1] + nz * fwd[2]) / nl), 3);
        const lum = clamp01((0.06 + 0.7 * diffuse + 0.5 * spec + 0.2 * rim) * gain(mat));
        const ch = SHADE.charAt(1 + Math.floor(lum * (SHADE.length - 2)));
        const i = r * cols + c;
        mats[i] = mat;
        lums[i] = lum;
        if (lit(mat)) glow[r]![c] = ch;
        else base[r]![c] = ch;
      }
    }
  };

  /** Where a world point lands, in columns and rows. */
  const project = (x: number, y: number, z: number): [number, number] => {
    const dx = x - tx;
    const dy = y - ty;
    const dz = z - tz;
    return [
      (dx * right[0] + dy * right[1] + dz * right[2]) / scale + cols / 2 - 0.5,
      -(dx * up[0] + dy * up[1] + dz * up[2]) / rowScale + rows / 2 - 0.5,
    ];
  };

  const inside = (c: number, r: number) => c >= 0 && c < cols && r >= 0 && r < rows;
  const empty = (c: number, r: number) => inside(c, r) && base[r]![c] === " " && glow[r]![c] === " ";
  /** Words or a mark, over whatever is there. */
  const put = (c: number, r: number, text: string, lit = false) => {
    c = Math.round(c);
    r = Math.round(r);
    for (let i = 0; i < text.length; i++) {
      if (!inside(c + i, r)) continue;
      base[r]![c + i] = lit ? " " : text.charAt(i);
      glow[r]![c + i] = lit ? text.charAt(i) : " ";
    }
  };
  /** A mark only where nothing is drawn yet: behind the solid. */
  const behind = (c: number, r: number, ch: string, lit = false) => {
    c = Math.round(c);
    r = Math.round(r);
    if (empty(c, r)) put(c, r, ch, lit);
  };
  const mat = (c: number, r: number) => (inside(c, r) ? mats[r * cols + c]! : -1);
  /** Light up a cell of the solid, keeping its shading. */
  const kindle = (c: number, r: number) => {
    if (!inside(c, r) || mats[r * cols + c]! < 0) return;
    const ch = base[r]![c] !== " " ? base[r]![c]! : glow[r]![c]!;
    base[r]![c] = " ";
    glow[r]![c] = ch;
  };
  /** A ring of dots spreading from a point, behind everything. */
  const ripple = (c0: number, r0: number, radius: number, ch: string, lit: boolean) => {
    for (let a = 0; a < Math.PI * 2; a += 0.5 / Math.max(1, radius)) {
      behind(c0 + Math.cos(a) * radius, r0 + Math.sin(a) * radius * CELL_ASPECT, ch, lit);
    }
  };
  const frame = (): Frame => ({
    base: base.map((row) => row.join("") + "\n").join(""),
    glow: glow.map((row) => row.join("") + "\n").join(""),
  });
  return { shoot, project, put, behind, mat, kindle, ripple, frame, lums };
}

/**
 * Pairing: a padlock turning slowly, shackle open, over the six digits the
 * PC is showing. As each is typed on the tablet it rises into the keyhole;
 * on the sixth the shackle swings down and snaps shut, the keyhole lights —
 * the PC's certificate is pinned — and a ring goes out. The code's two-minute
 * window runs down underneath.
 */
export function pair(t: number, cols = 58, rows = 18): Frame {
  const period = 7.6;
  const round = Math.floor(t / period);
  const s = t - round * period;
  const pin = Array.from({ length: 6 }, (_, i) => Math.min(9, Math.floor(hash(round * 17.3 + i * 5.1) * 10)));

  const TYPE = 0.7;
  const STEP = 0.32;
  const SHUT = TYPE + STEP * 6 + 0.35;
  const REOPEN = period - 0.8;

  const typed = s < TYPE ? 0 : Math.min(6, Math.floor((s - TYPE) / STEP) + 1);
  const closing = easeIn((s - SHUT) / 0.16);
  const open = s < REOPEN ? 1 - closing : easeOut((s - REOPEN) / 0.6);
  const lift = 0.26 * open;
  const swing = 1.15 * open;
  const flash = s >= SHUT + 0.12 && s < SHUT + 0.6;
  const pinned = s >= SHUT + 0.12 && s < REOPEN;

  const R = 0.38;
  const scene: Scene = (x, y, z) => {
    // Body, with the keyhole cut through it.
    const by = y + 0.42;
    let body = roundBox(x, by, z, 0.5, 0.36, 0.14, 0.22);
    const hy = y + 0.34;
    const hole = Math.min(len2(x, hy) - 0.12, Math.max(Math.abs(x) - 0.05, Math.abs(hy + 0.17) - 0.16));
    body = Math.max(body, -Math.max(hole, Math.abs(z) - 1));

    // The shackle: an arch on two legs, swung about its long left leg.
    let qx = x + R;
    let qz = z;
    [qx, qz] = rotate(qx, qz, swing);
    qx -= R;
    const qy = y - (0.08 + lift);
    let shackle: number;
    if (qy > 0) shackle = len2(len2(qx, qy) - R, qz) - 0.11;
    else {
      const leg = qx < 0 ? 0.42 + lift : 0.2;
      shackle = Math.max(len2(Math.abs(qx) - R, qz) - 0.11, -qy - leg);
    }
    if (shackle < body) return [shackle, 2];
    // The keyhole's walls glow once pinned, so mark them apart from the face.
    return [body, hole < 0.02 && Math.abs(z) < 0.5 ? 3 : 1];
  };

  const g = studio(cols, rows, {
    yaw: -0.4 + Math.sin(t * 0.6) * 0.45,
    pitch: 0.22,
    scale: 0.07,
    target: [0, -0.52, 0],
  });

  // A ring off the lock as it snaps.
  const [lc, lr] = g.project(0, -0.3, 0);
  if (s >= SHUT + 0.1 && s < SHUT + 1.1) {
    const age = s - SHUT - 0.1;
    g.ripple(lc, lr, 10 + age * 18, age < 0.12 ? "*" : ".", age < 0.6);
  }

  g.shoot(scene, (m) => (m === 2 && flash) || (m === 3 && pinned));

  // The code, in slots under the lock.
  const row = rows - 3;
  const c0 = Math.round(cols / 2 - 8);
  for (let i = 0; i < 6; i++) {
    const c = c0 + i * 3 + (i > 2 ? 1 : 0);
    const shown = i < typed && s < REOPEN;
    const fresh = shown && i === typed - 1 && s - (TYPE + i * STEP) < 0.3;
    g.put(c, row, shown ? String(pin[i]) : "_", fresh || (pinned && s < SHUT + 0.6));
    // Each digit, as it is typed, rises into the keyhole.
    const age = s - (TYPE + i * STEP);
    if (shown && age >= 0 && age < 0.3) {
      const p = easeOut(age / 0.3);
      const [kc, kr] = g.project(0, -0.34, 0.3);
      const pc = c + (kc - c) * p;
      const pr = row - 1 + (kr - row + 1) * p;
      g.put(pc, pr, "*", true);
      g.behind(c + (kc - c) * (p - 0.15), row - 1 + (kr - row + 1) * (p - 0.15), ".", true);
    }
  }

  // Underneath: the window running down, or what happened.
  if (pinned) g.put(cols / 2 - 6, rows - 1, "cert  pinned", s < SHUT + 1.4);
  else {
    const left = Math.ceil(17 * (1 - s / (period * 1.6)));
    g.put(c0, rows - 1, "-".repeat(left) + " ".repeat(17 - left));
  }
  return g.frame();
}

/** One automation, as the editor reads it. */
const SENTENCES: { word: string; rest: string; indent?: true }[] = [
  { word: "when", rest: "obs goes live" },
  { word: "if", rest: "mic is on" },
  { word: "repeat", rest: "3 times" },
  { word: "do", rest: "hue: flash", indent: true },
  { word: "do", rest: "scene: live" },
];

/**
 * Automations: a ring tumbling — the loop — beside the automation it stands
 * for, in the plain sentences the editor writes it in. A dry run walks the
 * lines; while it repeats, a lit bead runs round the ring once a lap, and the
 * run ends by saying nothing was sent.
 */
export function rail(t: number, cols = 58, rows = 18): Frame {
  const period = 7.2;
  const s = t % period;

  // When each line runs: the repeat's body three times over.
  const beats: { at: number; line: number; lap?: number }[] = [
    { at: 0.5, line: 0 },
    { at: 1.2, line: 1 },
  ];
  for (let lap = 1; lap <= 3; lap++) {
    beats.push({ at: 1.9 + (lap - 1) * 0.9, line: 2, lap });
    beats.push({ at: 2.3 + (lap - 1) * 0.9, line: 3, lap });
  }
  beats.push({ at: 4.7, line: 4 });
  const END = 5.4;

  let line = -1;
  let lap = 0;
  let since = 0;
  for (const beat of beats) {
    if (s < beat.at) break;
    since = beat.at;
    if (s < END) {
      line = beat.line;
      lap = beat.lap ?? lap;
    }
  }
  const working = line >= 0 && s - since < 0.35;

  // The loop: a ring tumbling slowly, a lit bead running round it once a lap.
  const R = 0.78;
  const ra = t * 0.45;
  const rb = 0.9 + Math.sin(t * 0.3) * 0.25;
  const looping = line === 2 || line === 3;
  const bead = looping ? Math.PI * 2 * ((s - beats[2]!.at) / 0.9) : -1;
  const scene: Scene = (x, y, z) => {
    let [px, py] = rotate(x, y, ra);
    let pz = z;
    [py, pz] = rotate(py, pz, rb);
    const d = len2(len2(px, pz) - R, py) - 0.34;
    if (bead < 0) return [d, 0];
    // Lit where the surface is near the bead's place on the ring.
    const bx = Math.cos(bead) * R;
    const bz = Math.sin(bead) * R;
    const near = len3(px - bx, py, pz - bz) < 0.5;
    return [d, near ? 1 : 0];
  };

  const g = studio(cols, rows, { yaw: 0, pitch: 0, scale: 0.082, target: [1.25, 0, 0] });
  g.shoot(scene, (m) => m === 1);
  if (working && !looping) {
    const [c, r] = g.project(0, 0, 0);
    g.ripple(c, r, 12 + (s - since) * 20, ".", true);
  }

  // The sentences, and the run's cursor walking them.
  const c0 = 31;
  const r0 = 3;
  SENTENCES.forEach((sentence, i) => {
    const r = r0 + i * 2;
    const c = c0 + (sentence.indent ? 2 : 0);
    const active = i === line;
    if (sentence.indent) g.put(c0, r, ":");
    if (i === 3 || i === 2) g.put(c0, r0 + 5, ":");
    if (active) g.put(c - 2, r, ">", true);
    g.put(c, r, sentence.word, active);
    const rest = i === 2 && lap > 0 && s < END ? `${lap} of 3` : sentence.rest;
    g.put(c + sentence.word.length + 1, r, rest, active && s - since < 0.4);
  });
  if (s >= END) g.put(c0, r0 + 11, "dry run: nothing sent", s < END + 1);
  else g.put(c0, r0 + 11, "dry run");
  return g.frame();
}

/** Smooth minimum, for shapes that flow into each other. */
function smin(a: number, b: number, k: number): number {
  const h = clamp01(0.5 + (0.5 * (b - a)) / k);
  return b + (a - b) * h - k * h * (1 - h);
}

/**
 * A jigsaw piece facing the camera: a square with a knob on the right and top
 * and a socket on the left and bottom, so a row of them interlocks. Extruded,
 * with its edges rounded.
 */
function piece(x: number, y: number, z: number): number {
  let d = roundBox(x, y, 0, 0.42, 0.42, 0, 0.08);
  d = smin(d, len2(x - 0.6, y) - 0.17, 0.06);
  d = smin(d, len2(x, y - 0.6) - 0.17, 0.06);
  d = Math.max(d, -(len2(x + 0.4, y) - 0.19));
  d = Math.max(d, -(len2(x, y + 0.4) - 0.19));
  const wx = d;
  const wz = Math.abs(z) - 0.12;
  return Math.min(Math.max(wx, wz), 0) + len2(Math.max(wx, 0), Math.max(wz, 0)) - 0.05;
}

/** Plugins to install, round after round, and what each asks for. */
const PIECES: { name: string; from: string; asks: string[] }[] = [
  { name: "hue", from: "repo", asks: ["network"] },
  { name: "discord", from: "repo", asks: ["network", "notifications"] },
  { name: "my-kit", from: "./my-kit", asks: ["filesystem", "processes"] },
  { name: "obs", from: "repo", asks: ["network"] },
];

/**
 * Installing a plugin: a jigsaw piece comes down from a reviewed repository
 * (or a linked folder) and hangs, turning, while a scan line reads it;
 * everything it asks for is listed beside it and ticked off, one by one, and
 * only then does it drop into the gap between its neighbours and lock in.
 */
export function install(t: number, cols = 58, rows = 18): Frame {
  const period = 5.8;
  const round = Math.floor(t / period);
  const s = t - round * period;
  // Wrapped both ways: a time before zero (a scene asked for its past) is a
  // negative round, and `%` keeps the sign.
  const plugin = PIECES[((round % PIECES.length) + PIECES.length) % PIECES.length]!;

  const ARRIVE = 0.8;
  const ASK = ARRIVE + 0.15;
  const EACH = 0.65;
  const AGREED = ASK + EACH * plugin.asks.length + 0.2;
  const LANDED = AGREED + 0.4;
  const LEAVE = period - 0.5;

  const W = 1.05; // a piece's pitch in the row
  const rowY = -0.62;
  const hang = 0.62;

  let py = 3 - easeOut(s / ARRIVE) * (3 - hang);
  // It turns while it hangs, settling face-on as it drops into the gap.
  let spin = s < ARRIVE ? 0 : (s - ARRIVE) * 1.7;
  if (s >= AGREED) {
    const p = easeIn((s - AGREED) / (LANDED - AGREED));
    py = hang + (rowY - hang) * p;
    spin = spin * (1 - p) + Math.round(spin / Math.PI) * Math.PI * p;
  }
  // At the end of the round the row lets go, ready for the next.
  const fade = s >= LEAVE ? easeIn((s - LEAVE) / 0.5) : 0;

  const scene: Scene = (x, y, z) => {
    // Its neighbours, already in place either side of the gap.
    let best: [number, number] = [piece(x < 0 ? x + W : x - W, y - rowY + fade * 3, z), 0];
    const [qx, qz] = rotate(x, z, spin);
    const d = piece(qx, y - py - fade * 3, qz);
    if (d < best[0]) best = [d, 1];
    return best;
  };

  const g = studio(cols, rows, { yaw: 0.5, pitch: 0.45, scale: 0.08, target: [0, -0.05, 0] });
  const flash = s >= LANDED && s < LANDED + 0.5;
  g.shoot(scene, (m) => m === 1 && flash, (m) => (m === 0 ? 0.75 : 1.2));

  // A scan line reading the hanging piece.
  if (s >= ASK && s < AGREED) {
    const [, top] = g.project(0, py + 0.66, 0);
    const [, bottom] = g.project(0, py - 0.5, 0);
    const r = Math.round(top + (((s - ASK) * 0.9) % 1) * (bottom - top));
    for (let c = 0; c < cols; c++) if (g.mat(c, r) === 1) g.kindle(c, r);
  }

  // Once in, its outline stays lit: the new piece among the old.
  if (s >= LANDED + 0.5 && fade === 0) {
    for (let r = 0; r < rows; r++) {
      for (let c = 0; c < cols; c++) {
        if (g.mat(c, r) !== 1) continue;
        if (g.mat(c - 1, r) !== 1 || g.mat(c + 1, r) !== 1 || g.mat(c, r - 1) !== 1 || g.mat(c, r + 1) !== 1) g.kindle(c, r);
      }
    }
  }

  // The snap: a ring out from the gap it filled.
  if (s >= LANDED && s < LANDED + 0.9) {
    const [c, r] = g.project(0, rowY, 0);
    const age = s - LANDED;
    g.ripple(c, r, 9 + age * 24, age < 0.35 ? "*" : ".", age < 0.6);
  }

  // Where it comes from, and what it asks for, ticked off one by one.
  const words = s >= 0.1 && s < LEAVE;
  if (words) {
    g.put(1, 1, plugin.from === "repo" ? "from repo" : "linked folder");
    g.put(1, 2, plugin.from === "repo" ? plugin.name : plugin.from, s < ARRIVE + 0.6);
  }
  plugin.asks.forEach((ask, i) => {
    const shown = ASK + EACH * i;
    if (s < shown || !words) return;
    const agreed = s >= shown + EACH * 0.6;
    const c = cols - 1 - (ask.length + 4);
    g.put(c, 1 + i, agreed ? "[x] " : "[ ] ", agreed && s < shown + EACH);
    g.put(c + 4, 1 + i, ask, s - shown < 0.25);
  });
  if (s >= LANDED && words) g.put(cols - 10, 1 + plugin.asks.length, "installed", s < LANDED + 0.8);
  return g.frame();
}

export const FIELDS: Record<FieldKind, (t: number, cols?: number, rows?: number) => Frame> = {
  rings,
  pair,
  faders,
  flow,
  rail,
  rain,
  install,
  scan,
  globe,
  pulse,
  typing,
  automate,
};

/** Full-frame static for "signal lost". */
export function noise(seed: number, cols: number, rows: number): string {
  const chars = " .:-=+*#";
  let out = "";
  for (let y = 0; y < rows; y++) {
    let line = "";
    for (let x = 0; x < cols; x++) {
      const r = hash(x * 12.9898 + y * 78.233 + seed * 3.17);
      line += chars.charAt(Math.floor(r * r * chars.length));
    }
    out += line + "\n";
  }
  return out;
}
