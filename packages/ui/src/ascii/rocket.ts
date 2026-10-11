import { hash, type Frame } from "./engine.ts";

interface Particle {
  x: number;
  y: number;
  vx: number;
  vy: number;
  life: number;
  grounded: boolean;
}

const BODY = ["    /\\", "   /  \\", "  |    |", "  | FD |", "  |    |", "  |    |", " /|    |\\", "/_|____|_\\", "   /__\\"];
const RAMP = " .:-=+*#%@";

/**
 * A launch: exhaust particles fall from the nozzle, hit the pad, and roll out
 * sideways into cloud banks, accumulated into a density grid and drawn through
 * a ramp. Rings expand behind the rocket; a perspective grid recedes from the
 * pad.
 *
 * Stateful by nature — particles carry over between frames — so it is a class,
 * and `random` is injectable to keep it testable.
 */
export class RocketSim {
  private particles: Particle[] = [];
  private readonly random: () => number;
  readonly cols: number;
  readonly rows: number;

  constructor(cols = 150, rows = 64, random: () => number = Math.random) {
    this.cols = cols;
    this.rows = rows;
    this.random = random;
  }

  /** Advance one step and draw. `lift` is 0 on the pad, 1 well clear of it. */
  step(t: number, lift: number): Frame {
    const W = this.cols;
    const H = this.rows;
    const ground = H - 9;
    const cx = Math.floor(W / 2);
    const ry = Math.round(ground - 9 - lift * (H * 0.53));
    const rnd = this.random;

    const thrust = lift > 0.02 ? 1 : 0.35;
    for (let i = 0; i < 26 * thrust; i++) {
      this.particles.push({
        x: cx + (rnd() - 0.5) * 2.2,
        y: ry + 7,
        vx: (rnd() - 0.5) * 0.5,
        vy: 0.9 + rnd() * 1.1,
        life: 1,
        grounded: false,
      });
    }

    const density = new Float32Array(W * H);
    const next: Particle[] = [];
    for (const p of this.particles) {
      p.x += p.vx;
      p.y += p.vy;
      if (!p.grounded && p.y >= ground) {
        p.grounded = true;
        p.y = ground - rnd() * 3;
        p.vx = (rnd() < 0.5 ? -1 : 1) * (0.7 + rnd() * 2.2);
        p.vy = -rnd() * 0.12;
      }
      if (p.grounded) {
        p.vx *= 0.985;
        p.vy *= 0.97;
        p.life -= 0.006;
      } else p.life -= 0.02;
      if (p.life <= 0 || p.x < 0 || p.x >= W || p.y < 0 || p.y >= H) continue;
      next.push(p);
      const ix = Math.floor(p.x);
      const iy = Math.floor(p.y);
      density[iy * W + ix] += p.life * (p.grounded ? 0.55 : 0.8);
      if (p.grounded && iy > 0) density[(iy - 1) * W + ix] += p.life * 0.3;
    }
    this.particles = next.length > 3200 ? next.slice(next.length - 3200) : next;

    let base = "";
    let glow = "";
    for (let y = 0; y < H; y++) {
      let lb = "";
      let lg = "";
      for (let x = 0; x < W; x++) {
        let ch = " ";
        let g = " ";
        const line = BODY[y - (ry - 2)];
        const bx = x - (cx - 4);
        if (line && bx >= 0 && bx < line.length && line.charAt(bx) !== " ") ch = line.charAt(bx);
        else {
          const d = density[y * W + x]!;
          if (d > 0.05) {
            const ci = Math.min(RAMP.length - 1, Math.floor(d * 0.85 * RAMP.length));
            if (y > ry + 6 && y < ry + 12 && Math.abs(x - cx) < 3) g = RAMP.charAt(Math.max(5, ci));
            else ch = RAMP.charAt(Math.max(1, ci));
          } else if (y >= ground) {
            const k = (x - cx) / (y - ground + 2);
            const fk = Math.abs(k * 2.2 - Math.round(k * 2.2));
            if (fk < (0.06 * (y - ground + 2)) / 3) ch = ".";
            else if ((y - ground) % 3 === 0 && (x + y) % 2 === 0) ch = ".";
          } else {
            const dx = (x - cx) * 0.6;
            const dy = y - (ry - 12);
            const r = Math.hypot(dx, dy);
            let ring = (r / 5 - t * 0.35) % 1;
            if (ring < 0) ring += 1;
            if (ring < 0.06 && r > 8 && r < 70) ch = ".";
            const s = hash(x * 3.1 + y * 17.7);
            if (s > 0.993) ch = Math.sin(t * 3 + s * 90) > 0.5 ? "*" : ".";
          }
        }
        lb += g !== " " ? " " : ch;
        lg += g;
      }
      base += lb + "\n";
      glow += lg + "\n";
    }
    return { base, glow };
  }
}
