/**
 * Development: the desktop app.
 *
 * One command that opens the window. It does three things in order:
 *
 * 1. starts the window's UI on Vite, because a debug build of the shell loads
 *    its interface from `devUrl` (`127.0.0.1:5174`) rather than from a bundle;
 * 2. waits for that server to actually answer, because the shell is a *native*
 *    window and would otherwise race the dev server and come up blank;
 * 3. runs the shell, and stops the dev server when the shell exits, so closing
 *    the app does not leave a stray Vite process holding the port.
 *
 * The headless agent is a different command (`pnpm dev:agent`). It is still the
 * right thing for a server with no window, but it is not this: a terminal banner
 * is not a desktop app.
 */
import { spawn, type ChildProcess } from "node:child_process";

import { fail, isPortTaken, pnpmCommand, repoRoot, stop } from "./env.ts";

/** Where the window's UI is served. Must match `build.devUrl` in tauri.conf.json. */
const DEV_URL = "http://127.0.0.1:5174";

/** The agent's default port, from `Settings` in flexdeck-server. */
const AGENT_PORT = 7878;

/**
 * The portal the window dials, from `DEFAULT_PORT` in flexdeck-cloud.
 *
 * Passed to the window rather than left to the database, because a desktop that
 * cannot name a portal opens no tunnel — the "web access" toggle would be a
 * button that only ever fails. Override with `pnpm dev:desktop -- --cloud <url>`.
 */
const DEFAULT_CLOUD = "ws://127.0.0.1:8787/tunnel";
const CLOUD_PORT = 8787;

/** How long to give Vite before giving up on it. */
const READY_TIMEOUT_MS = 60_000;

async function main(): Promise<void> {
  await assertPortFree();

  const cloud = cloudUrl();
  if (cloud === DEFAULT_CLOUD) {
    await warnIfCloudMissing();
  }

  const ui = startUi();

  try {
    await waitForUi();
  } catch (error) {
    stop(ui);
    fail(
      [
        error instanceof Error ? error.message : String(error),
        `The window's UI did not come up on ${DEV_URL}, so the window would have opened blank.`,
      ].join("\n"),
    );
  }

  const shell = spawn("cargo", ["run", "-p", "flexdeck-desktop", "--", "--cloud", cloud], {
    cwd: repoRoot,
    stdio: "inherit",
  });

  shell.on("error", (error) => {
    stop(ui);
    fail(`Could not start the window: ${error.message}`);
  });

  shell.on("exit", (code) => {
    stop(ui);
    process.exitCode = code ?? 1;
  });
}

/**
 * Refuse to start if another agent already holds the port.
 *
 * The window would otherwise open, look healthy, and serve nothing — the failure
 * is real but it is invisible from inside the app. Naming it here is the whole
 * point: the usual cause is a `dev:agent` left running in another terminal.
 */
async function assertPortFree(): Promise<void> {
  const taken = await isPortTaken(AGENT_PORT);

  if (taken) {
    fail(
      [
        `Something is already using port ${AGENT_PORT}, so the window's tablet listener could not start.`,
        "The usual cause is the headless agent still running in another terminal —",
        `stop it (Ctrl+C) and run this again.`,
      ].join("\n"),
    );
  }
}

/**
 * The portal this window should dial: the local development cloud by default,
 * overridable with `--cloud <url>` before it is baked into the settings.
 */
function cloudUrl(): string {
  const args = process.argv.slice(2);
  const index = args.indexOf("--cloud");

  return (index === -1 ? undefined : args[index + 1]) ?? DEFAULT_CLOUD;
}

/**
 * Say so when nothing is serving the portal.
 *
 * Not fatal: the window is still useful for pairing tablets with no portal at
 * all. But the failure it prevents is opaque — the window opens, web access is
 * switched on, and the tunnel simply never comes up.
 */
async function warnIfCloudMissing(): Promise<void> {
  if (await isPortTaken(CLOUD_PORT)) {
    return;
  }

  console.warn(
    [
      `Nothing is listening on port ${CLOUD_PORT}, so this window has no portal to reach.`,
      "Run `pnpm start` in another terminal to link this computer to the website.",
    ].join("\n"),
  );
}

function startUi(): ChildProcess {
  // Reached through pnpm so the package's own `dev` script stays the single
  // definition of how the UI starts, exactly as the other dev scripts do.
  const { command, args, shell } = pnpmCommand(["--filter", "@flexdeck/desktop", "dev"]);

  const child = spawn(command, args, {
    cwd: repoRoot,
    stdio: "inherit",
    shell,
  });

  child.on("error", (error) => {
    fail(`Could not start the UI server: ${error.message}`);
  });

  return child;
}

/** Poll until Vite answers, or give up. */
async function waitForUi(): Promise<void> {
  const deadline = Date.now() + READY_TIMEOUT_MS;

  while (Date.now() < deadline) {
    try {
      const response = await fetch(DEV_URL);
      if (response.ok) {
        return;
      }
    } catch {
      // Not up yet. That is the expected case until it is.
    }

    await new Promise((resolve) => setTimeout(resolve, 200));
  }

  throw new Error(`Timed out after ${READY_TIMEOUT_MS / 1000}s waiting for ${DEV_URL}.`);
}

void main();
