/**
 * Development: the web app — the portal UI and the cloud API behind it.
 *
 * `dev:server` (`--api-only`) brings up the cloud API alone — for running the
 * portal some other way, or for working on the API and the tunnel.
 *
 * `dev:web` brings up BOTH halves, because the portal on its own is a UI whose
 * every call fails. Vite serves the UI on 127.0.0.1:5173 and proxies `/api` to
 * the cloud on 127.0.0.1:8787, so with nothing behind that proxy the browser gets
 * `502 Bad Gateway` for every request — which reads like a bug in the portal
 * rather than a process nobody started.
 *
 * # This is the remote side of the product
 *
 * The API here is `flexdeck-cloud`: the service that runs on *our* server, not on
 * the user's machine. Binding it to loopback is a development convenience, not a
 * different topology — and the topology is the thing to keep in mind.
 *
 * A cloud service cannot dial a desktop's local address. It has no route to
 * `127.0.0.1:23539` on someone else's computer, and must never be designed as
 * though it did. So the desktop is the side that connects: it opens an
 * **outbound** tunnel to this cloud, and the portal's requests travel back *down*
 * that connection. Nothing here, and nothing in the cloud, reaches out to a
 * desktop.
 *
 * The desktop is deliberately not started here — it stands in for a different
 * machine, so it stays a different command. It also has to be *told* which cloud
 * to dial, and today only the agent binary can do that:
 *
 *   pnpm dev:agent -- --cloud ws://127.0.0.1:8787/tunnel
 *
 * The value is persisted, so it is a one-time step. The desktop *app*
 * (`pnpm dev:desktop`) has neither a flag nor a setting for it yet, so until an
 * agent has written it the app reports web access as unavailable.
 */
import { spawn, type ChildProcess } from "node:child_process";

import { fail, isPortTaken, pnpmCommand, repoRoot, stop } from "./env.ts";

/**
 * The cloud's default port.
 *
 * `DEFAULT_PORT` in `crates/flexdeck-cloud/src/bin/flexdeck-cloud.rs` and the
 * proxy target in `apps/portal/vite.config.ts`. If it changes, that file changes
 * with it.
 */
const API_PORT = 8787;

/** The API as this machine reaches it, and a route that proves it answered. */
const API_URL = `http://127.0.0.1:${API_PORT}`;
const API_PROBE_PATH = "/api/health";

/** The portal dev server: `server.host`/`server.port` in `apps/portal/vite.config.ts`. */
const UI_PORT = 5173;
const UI_URL = `http://127.0.0.1:${UI_PORT}`;

/** Generous, because a first run has to compile the whole Rust workspace. */
const API_READY_TIMEOUT_MS = 300_000;

/** Vite is quick, but a cold dependency scan is not instant. */
const UI_READY_TIMEOUT_MS = 60_000;

/**
 * How long one readiness request may hang before it counts as "not up yet".
 *
 * Needed because a connection can be *accepted* by a socket that never answers,
 * and an un-timed `fetch` would then wait forever instead of retrying.
 */
const REQUEST_TIMEOUT_MS = 2000;

/** `dev:server`: the cloud API without the portal UI. */
const API_ONLY = process.argv.includes("--api-only");

async function main(): Promise<void> {
  await assertFree(API_PORT, "the cloud API");
  if (!API_ONLY) await assertFree(UI_PORT, "the portal UI");

  const children: ChildProcess[] = [];
  let stopping = false;

  /**
   * Stop every server, once, whichever one ended first.
   *
   * The flag is load-bearing: without it the two `exit` handlers `stop()` each
   * other, and the second overwrites the exit code the first was reporting — so
   * a crash can be passed off as a clean exit.
   */
  const shutdown = (code: number | null): void => {
    if (stopping) {
      return;
    }

    stopping = true;

    for (const child of children) {
      stop(child);
    }

    process.exitCode = code ?? 1;
  };

  // Spawned and wired up BEFORE anything is waited for. A child that dies during
  // startup — a compile error, a port taken after the check — must end this
  // script at once, not be waited out for the full budget in silence.
  const api = startApi();
  children.push(api);
  api.on("exit", (code) => shutdown(code));

  try {
    await waitForApi(api);
  } catch (error) {
    shutdown(1);
    fail(describe(error));
  }

  if (API_ONLY) {
    report();
    return;
  }

  const ui = startUi();
  children.push(ui);
  ui.on("exit", (code) => shutdown(code));

  try {
    await waitForUi(ui);
  } catch (error) {
    shutdown(1);
    fail(describe(error));
  }

  report();
}

/**
 * The cloud: the API and the tunnel endpoint. Never the built portal here:
 * the portal is Vite's, live, and a build left in `apps/portal/dist` would be
 * served on the API's port as an old copy that looks exactly like the real one.
 */
function startApi(): ChildProcess {
  const child = spawn("cargo", ["run", "-p", "flexdeck-cloud", "--", "--no-portal"], {
    cwd: repoRoot,
    stdio: "inherit",
  });

  child.on("error", (error) => {
    fail(`Could not start the cloud API: ${error.message}`);
  });

  return child;
}

/** Vite, reached through pnpm so the package's own `dev` script stays the one definition. */
function startUi(): ChildProcess {
  const { command, args, shell } = pnpmCommand(["--filter", "@flexdeck/portal", "dev"]);

  const child = spawn(command, args, {
    cwd: repoRoot,
    stdio: "inherit",
    shell,
  });

  child.on("error", (error) => {
    fail(`Could not start the portal UI: ${error.message}`);
  });

  return child;
}

/**
 * Refuse to start on a port something already holds, and say how to clear it.
 *
 * Naming only the *likely* cause is not enough: the person hitting this has a
 * process they cannot see, and has to go hunting for it. The exact commands are
 * cheaper than that hunt, and they are the same on every run.
 */
async function assertFree(port: number, what: string): Promise<void> {
  if (!(await isPortTaken(port))) {
    return;
  }

  fail(
    [
      `Port ${port} is already in use, so ${what} could not start.`,
      "Something from an earlier run is usually still holding it. Either use that",
      "one, or stop it:",
      "",
      `  PowerShell:  Get-NetTCPConnection -LocalPort ${port} -State Listen |`,
      "               ForEach-Object { Stop-Process -Id $_.OwningProcess -Force }",
      `  macOS/Linux: lsof -ti tcp:${port} | xargs kill`,
      "",
      "Only one process can hold a port, so this refuses rather than starting a",
      "second copy that looks healthy and serves nothing.",
    ].join("\n"),
  );
}

/**
 * Poll until `url` answers an HTTP request, or give up.
 *
 * A real request rather than a bare TCP connect, because an accepted connection
 * only proves that *something* holds the port — not that it is the server being
 * waited for. Any status counts as an answer: `GET /api/health` replies 200
 * as soon as the cloud is serving, with no credential needed.
 */
async function waitForRequest(
  url: string,
  what: string,
  timeoutMs: number,
  child: ChildProcess,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    // A child that has already exited will never answer, so give up now rather
    // than after the full budget. That is the difference between seeing a
    // compile error in seconds and staring at nothing for five minutes.
    if (child.exitCode !== null) {
      throw new Error(
        `${what} stopped before it was ready (exit code ${child.exitCode}); see its output above.`,
      );
    }

    try {
      await fetch(url, { signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS) });
      return;
    } catch {
      // Not up yet. That is the expected case until it is.
    }

    await delay(200);
  }

  throw new Error(`Timed out after ${timeoutMs / 1000}s waiting for ${what} at ${url}.`);
}

function waitForApi(child: ChildProcess): Promise<void> {
  return waitForRequest(
    `${API_URL}${API_PROBE_PATH}`,
    "the cloud API",
    API_READY_TIMEOUT_MS,
    child,
  );
}

function waitForUi(child: ChildProcess): Promise<void> {
  return waitForRequest(UI_URL, "the portal UI", UI_READY_TIMEOUT_MS, child);
}

function report(): void {
  console.log();
  console.log(API_ONLY ? "flexdeck server" : "flexdeck web");
  if (!API_ONLY) console.log(`  portal   ${UI_URL}`);
  console.log(`  api      ${API_URL}`);
  console.log();
  console.log("  The cloud never dials a desktop — a desktop dials out to it.");
  console.log("  To connect one, hand an agent the tunnel URL once:");
  console.log(`    pnpm dev:agent -- --cloud ws://127.0.0.1:${API_PORT}/tunnel`);
  console.log("  It is persisted. Stop the dev window first: both bind the same port.");
  console.log("  (The desktop app itself cannot be pointed at a portal yet.)");
  console.log();
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

void main();
