/**
 * Will this plugin work in FlexDeck? Asked of FlexDeck itself.
 *
 * `flexdeck-plugin verify` packs the plugin and hands it to the FlexDeck on
 * this computer (`flexdeck verify-plugin`), which runs it exactly as it would
 * after an install — the same host, deadlines and rules — and checks it at
 * every step: package and catalogue, gallery keys, every action's examples,
 * its values, feeds and settings, that it keeps to its own namespace and
 * declared events, that it writes nothing but the protocol, and that it stops
 * cleanly. The SDK only finds FlexDeck and shows the answer; nothing here
 * re-implements the rules, so they cannot drift from the app's.
 */
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";

export interface ConformanceCheck {
  name: string;
  ok: boolean;
  detail: string;
}

export interface ConformanceReport {
  plugin: string;
  runtime: string;
  checks: ConformanceCheck[];
}

const EXE = process.platform === "win32" ? "flexdeck.exe" : "flexdeck";

/**
 * The FlexDeck to ask: `FLEXDECK_BIN`; a build in a FlexDeck checkout above
 * `from`; `flexdeck` on the PATH; then where the app installs.
 */
export function findFlexdeck(from: string = process.cwd(), env: NodeJS.ProcessEnv = process.env): string | null {
  if (env.FLEXDECK_BIN) return existsSync(env.FLEXDECK_BIN) ? env.FLEXDECK_BIN : null;
  for (let dir = resolve(from); ; dir = dirname(dir)) {
    for (const build of ["debug", "release"]) {
      const candidate = join(dir, "target", build, EXE);
      if (existsSync(candidate)) return candidate;
    }
    if (dirname(dir) === dir) break;
  }
  for (const dir of (env.PATH ?? env.Path ?? "").split(delimiter)) {
    if (dir && existsSync(join(dir, EXE))) return join(dir, EXE);
  }
  const installed =
    process.platform === "win32"
      ? [env.LOCALAPPDATA && join(env.LOCALAPPDATA, "flexdeck", EXE), env.ProgramFiles && join(env.ProgramFiles, "flexdeck", EXE)]
      : process.platform === "darwin"
        ? ["/Applications/flexdeck.app/Contents/MacOS/flexdeck"]
        : ["/usr/bin/flexdeck", "/usr/local/bin/flexdeck"];
  return installed.find((path): path is string => Boolean(path) && existsSync(path as string)) ?? null;
}

/** Ask `flexdeck` whether the package at `file` (or a package folder) will work. */
export function conform(file: string, options: { flexdeck: string; runActions?: boolean }): ConformanceReport {
  const scratch = mkdtempSync(join(tmpdir(), "flexdeck-verify-"));
  const out = join(scratch, "report.json");
  try {
    const args = ["verify-plugin", resolve(file), "--out", out, ...(options.runActions ? ["--run-actions"] : [])];
    const run = spawnSync(options.flexdeck, args, { encoding: "utf8", timeout: 120_000, windowsHide: true });
    if (!existsSync(out)) {
      throw new Error(`${options.flexdeck} could not verify it: ${(run.stderr || run.error?.message || "no report").trim()}`);
    }
    return JSON.parse(readFileSync(out, "utf8")) as ConformanceReport;
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

/** The report as a person reads it. */
export function describe(report: ConformanceReport): string {
  const lines = [`${report.plugin} (${report.runtime})`];
  for (const check of report.checks) lines.push(`${check.ok ? "  ok  " : "  FAIL"}  ${check.name}${check.detail ? ` — ${check.detail}` : ""}`);
  const failed = report.checks.filter((check) => !check.ok).length;
  lines.push("", failed === 0 ? "It will work in FlexDeck." : `${failed} check${failed === 1 ? "" : "s"} failed: fix ${failed === 1 ? "it" : "them"} before anyone installs it.`);
  return lines.join("\n");
}

export const passed = (report: ConformanceReport): boolean => report.checks.every((check) => check.ok);
