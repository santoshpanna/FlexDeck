/**
 * Development: the headless agent.
 *
 * This is the server on its own: it seeds, pairs, serves and logs, with no
 * window. It is the right thing for a machine you reach over SSH, or for
 * watching the protocol in a terminal.
 *
 * The desktop *app* is `pnpm dev:desktop` — that one opens the window, and the
 * window hosts this same agent in its own process.
 *
 * Its banner carries the certificate fingerprint and a pairing PIN, which is the
 * quickest way to get a tablet connected when no window is open.
 *
 * Extra arguments pass through to the agent:
 *
 *   pnpm dev:agent -- --port 9000
 *
 * Stop this before running `cargo build`: on Windows a running binary holds a
 * lock on `target\debug\flexdeck.exe`, so the build fails with "Access is
 * denied" — and a failed build leaves the *previous* binary in place, to be
 * smoke-tested by mistake. It also holds the port the window needs.
 */
import { run } from "./env.ts";

run("cargo", ["run", "-p", "flexdeck-agent", "--", ...process.argv.slice(2)]);
