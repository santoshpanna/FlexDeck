/**
 * Shared ground for the development scripts.
 *
 * The three `dev:*` scripts exist so that starting each piece of flexdeck is one
 * command rather than a paragraph of PowerShell that has to be remembered and
 * retyped. Everything platform-specific lives here, so each script stays a short
 * recipe.
 *
 * Paths are derived from this file's location rather than hard-coded, so the
 * scripts work wherever the repository is checked out. The JDK and the Android
 * SDK themselves live under `.tools/`, which is gitignored: they are large, and
 * a fresh clone is expected to fetch them (see the README).
 */
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import { connect } from "node:net";
import { delimiter, join } from "node:path";
import { fileURLToPath } from "node:url";

/** The repository root, derived from this file's location. */
export const repoRoot = fileURLToPath(new URL("../../", import.meta.url));

const tools = join(repoRoot, ".tools");
const jdk17 = join(tools, "jdk-17");
const androidSdk = join(tools, "android-sdk");

export interface RunOptions {
  /** Environment for the child. Defaults to this process's own. */
  env?: NodeJS.ProcessEnv;
  /** Whether to go through a shell. Needed only for `.cmd` shims on Windows. */
  shell?: boolean;
}

/** Report a problem the way a person can act on, and stop. */
export function fail(message: string): never {
  console.error(message);
  process.exit(1);
}

/**
 * Run a command with this terminal's own stdio, and exit with its status.
 *
 * `stdio: "inherit"` rather than piping, because these are development servers:
 * their colour and their progress output is the point, and a pipe would hold
 * that output until the process ended, which for a server is never.
 *
 * Signals are not forwarded by hand. A Ctrl+C at the terminal goes to every
 * process attached to the console, so the child sees it without help — and on
 * Windows `child.kill("SIGINT")` is not supported anyway.
 */
export function run(command: string, args: string[], options: RunOptions = {}): void {
  const child = spawn(command, args, {
    cwd: repoRoot,
    stdio: "inherit",
    env: options.env ?? process.env,
    shell: options.shell ?? false,
  });

  child.on("error", (error) => {
    fail(`Could not start \`${command}\`: ${error.message}`);
  });

  child.on("exit", (code, signal) => {
    // A signal death carries no exit code, and reporting success for it would
    // let a killed build pass a `&&` chain.
    process.exitCode = signal === null ? (code ?? 1) : 1;
  });
}

/**
 * The command line that runs pnpm, given how this process was started.
 *
 * Called through `pnpm run …`, `npm_execpath` names pnpm's own entry point, so it
 * can be run with this same Node binary: no shell, so nothing to quote and
 * nothing to escape, and it works on Windows where `pnpm` is a `.cmd` shim that
 * Node refuses to execute directly.
 *
 * Run directly instead (`node scripts/startup/web.ts`), there is no such pointer
 * and a shell is the only way to reach the shim. The command is pre-joined into
 * a single string for that case: handing an args array to `shell: true` makes
 * Node concatenate the arguments without escaping them, which it now deprecates
 * for exactly that reason.
 *
 * Returned rather than spawned, so a script that runs several servers at once can
 * start pnpm without {@link run}'s "this process ends when the child does"
 * behaviour.
 */
export function pnpmCommand(args: string[]): {
  command: string;
  args: string[];
  shell: boolean;
} {
  const pnpm = process.env.npm_execpath;
  if (pnpm) {
    return { command: process.execPath, args: [pnpm, ...args], shell: false };
  }

  return { command: ["pnpm", ...args].join(" "), args: [], shell: true };
}

/**
 * Whether something is already listening on a loopback port.
 *
 * Both loopback families are probed, because a server bound only to `::1` is
 * invisible to an IPv4 probe. This repo has already been bitten once by Node
 * resolving `localhost` to `::1` (see the note in `apps/portal/vite.config.ts`),
 * and a guard that misses a listener is worse than no guard: it prints nothing
 * and the failure surfaces later as something else entirely.
 *
 * Any failure to connect counts as "nothing there": connection refused on
 * Windows, and a timeout is treated the same way rather than hanging.
 */
export async function isPortTaken(port: number): Promise<boolean> {
  return (await canConnect("127.0.0.1", port)) || (await canConnect("::1", port));
}

function canConnect(host: string, port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const probe = connect({ host, port });
    probe.setTimeout(1000);
    probe.on("connect", () => {
      probe.destroy();
      resolve(true);
    });
    probe.on("error", () => resolve(false));
    probe.on("timeout", () => {
      probe.destroy();
      resolve(false);
    });
  });
}

/**
 * Stop a dev server without leaving an orphan holding the port.
 *
 * A package script runs its server as a *grandchild* (node -> pnpm -> vite or
 * cargo -> the server), so killing the direct child can leave the server running.
 * The next run then fails with "port already in use" for a reason that looks
 * nothing like this one.
 *
 * Windows is the platform these scripts are developed and verified on, and
 * `/T` there kills the whole tree. On POSIX this signals the DIRECT child only,
 * so a grandchild may survive. Taking a process group down properly needs
 * `detached: true` at spawn time plus a parent-side SIGINT handler — a change to
 * how every script spawns — and an interactive `Ctrl+C` already reaches the whole
 * console process group, which is the case that matters day to day.
 */
export function stop(child: ChildProcess): void {
  if (child.exitCode !== null || child.signalCode !== null) {
    return;
  }

  if (process.platform === "win32" && child.pid !== undefined) {
    spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    return;
  }

  child.kill();
}

/**
 * The environment Gradle needs: `JAVA_HOME`, the JDK on `PATH`, and `ANDROID_HOME`.
 *
 * `JAVA_HOME` matters more than the `PATH` entry — Gradle picks its JVM from it
 * alone — and this machine's system Java is an 8 *JRE*, which cannot run the
 * build at all. That is the whole reason the toolchain is pinned here.
 *
 * A missing toolchain is reported here rather than passed through, because the
 * alternative is Gradle's own "SDK location not found" several screens long,
 * which never mentions that nothing is installed.
 */
export function androidEnvironment(): NodeJS.ProcessEnv {
  const missing: string[] = [];
  if (!existsSync(jdk17)) missing.push(jdk17);
  if (!existsSync(androidSdk)) missing.push(androidSdk);

  if (missing.length > 0) {
    fail(
      [
        "The Android toolchain is not installed, so the app cannot be built.",
        ...missing.map((path) => `  missing: ${path}`),
        "The SDK and JDK are deliberately not committed. See the README for how",
        "to fetch them, or run one of the other dev:* scripts instead.",
      ].join("\n"),
    );
  }

  const path = [join(jdk17, "bin"), process.env.PATH ?? ""].join(delimiter);

  return { ...process.env, JAVA_HOME: jdk17, PATH: path, ANDROID_HOME: androidSdk };
}
