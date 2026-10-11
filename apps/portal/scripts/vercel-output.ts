/**
 * Package a built portal (`dist/`) as a Vercel prebuilt deployment.
 *
 * Writes `.vercel/output/` at the repository root in Vercel's Build Output API
 * shape, so CI deploys exactly the bundle it built and tested — Vercel never
 * runs a build of its own, and needs no knowledge of this pnpm workspace.
 *
 * The static host serves the PUBLIC pages. The editor and its API stay on the
 * cloud server (same origin is what its session cookie needs — see
 * `crates/flexdeck-cloud/src/portal.rs`), so `/config` redirects to the
 * editor's root and `/api` to its API. Everything else falls back to
 * `index.html`: the app owns its routes.
 *
 * Security headers mirror the cloud's (`with_security_headers` in
 * `crates/flexdeck-cloud/src/http.rs`), including a script-src that allows the
 * inline scripts `index.html` carries by hash and nothing else inline.
 *
 * Run after `vite build`, with the editor's origin:
 *   VITE_EDITOR_URL=https://app.flex-deck.com node apps/portal/scripts/vercel-output.ts
 */
import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const portal = join(dirname(fileURLToPath(import.meta.url)), "..");
const dist = join(portal, "dist");
const output = join(portal, "..", "..", ".vercel", "output");

const editor = (process.env.VITE_EDITOR_URL ?? "").replace(/\/+$/, "");
if (!/^https:\/\//.test(editor)) {
  console.error("vercel-output: set VITE_EDITOR_URL to the editor's https origin (the same value the build used).");
  process.exit(1);
}
if (!existsSync(join(dist, "index.html"))) {
  console.error(`vercel-output: no build at ${dist} — run the portal build first.`);
  process.exit(1);
}

/** The bodies of `<script>` elements with no `src` — the same rule the cloud hashes by. */
function inlineScripts(html: string): string[] {
  return [...html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/g)].filter(([, attributes]) => !attributes.includes("src=")).map(([, , body]) => body);
}

const html = readFileSync(join(dist, "index.html"), "utf8");
const hashes = inlineScripts(html)
  .map((script) => ` 'sha256-${createHash("sha256").update(script).digest("base64")}'`)
  .join("");

const csp =
  `default-src 'self'; script-src 'self'${hashes}; style-src 'self' 'unsafe-inline'; ` +
  "img-src 'self' data:; connect-src 'self' http://127.0.0.1:* http://localhost:* ws://127.0.0.1:* ws://localhost:*; " +
  "object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

const config = {
  version: 3,
  routes: [
    // The editor lives at the cloud's root, and its API beside it.
    { src: "^/config(/.*)?$", status: 308, headers: { Location: `${editor}/` } },
    { src: "^/api(/.*)?$", status: 308, headers: { Location: `${editor}/api$1` } },
    // Hashed file names never change content, so they cache forever.
    { src: "^/assets/(.*)$", headers: { "Cache-Control": "public, max-age=31536000, immutable" }, continue: true },
    {
      src: "^/(.*)$",
      headers: {
        "Content-Security-Policy": csp,
        "X-Content-Type-Options": "nosniff",
        "X-Frame-Options": "DENY",
        "Referrer-Policy": "no-referrer",
      },
      continue: true,
    },
    { handle: "filesystem" },
    { src: "^/(.*)$", dest: "/index.html" },
  ],
};

rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });
cpSync(dist, join(output, "static"), { recursive: true });
writeFileSync(join(output, "config.json"), `${JSON.stringify(config, null, 2)}\n`);
console.log(`vercel-output: ${output} (editor at ${editor}, ${hashes ? hashes.trim().split(" ").length : 0} inline script hash(es))`);
