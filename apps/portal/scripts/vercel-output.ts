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
 * editor's root and `/api` to its API. Each public page is prerendered from
 * the server build (`build:ssr`) with its own title, description, canonical URL
 * and structured data, beside `sitemap.xml`, `robots.txt` and `llms.txt`. Anything else is
 * a real 404, so a stray URL is never indexed as a copy of the home page.
 *
 * Security headers mirror the cloud's (`with_security_headers` in
 * `crates/flexdeck-cloud/src/http.rs`), including a script-src that allows the
 * inline scripts `index.html` carries by hash and nothing else inline.
 *
 * Run after `vite build` and `build:ssr`, with the editor's origin:
 *   VITE_EDITOR_URL=https://app.flex-deck.com node apps/portal/scripts/vercel-output.ts
 */
import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { ASSETS, REPO_URL } from "../src/common/release/release.lib.ts";
import {
  SITE_ORIGIN,
  breadcrumbs,
  headTags,
  publicPages,
  llmsTxt,
  robots,
  sitemap,
  softwareApplication,
  website,
  type DocSummary,
} from "../src/common/site/seo.ts";

const portal = join(dirname(fileURLToPath(import.meta.url)), "..");
const dist = join(portal, "dist");
const serverEntry = join(portal, "dist-ssr", "entry-server.js");
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
if (!existsSync(serverEntry)) {
  console.error(`vercel-output: no server build at ${serverEntry} — run the portal's build:ssr first.`);
  process.exit(1);
}
const site = (process.env.SITE_URL ?? SITE_ORIGIN).replace(/\/+$/, "");
if (!/^https:\/\//.test(site)) {
  console.error("vercel-output: SITE_URL must be the public site's https origin.");
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

const server = (await import(pathToFileURL(serverEntry).href)) as {
  render: (path: string) => string;
  DOCS: DocSummary[];
  latest: { version: string } | null;
};

const pages = publicPages(server.DOCS);
const home = pages.find((page) => page.path === "/")!;
const image = `${site}/icon-512.png`;
const app = softwareApplication({
  origin: site,
  image,
  repository: REPO_URL,
  downloadUrl: ASSETS.windows.url,
  version: server.latest?.version ?? null,
  description: home.description,
});

function fill(template: string, needle: string, value: string): string {
  if (!template.includes(needle)) throw new Error(`vercel-output: index.html has no ${needle}`);
  return template.replace(needle, () => value);
}

const prerendered = pages.map((page) => {
  const structuredData =
    page.path === "/" ? [website(site), app] : [breadcrumbs(site, [{ name: "flexdeck", path: "/" }, { name: page.title.split(" · ")[0], path: page.path }])];
  const head = headTags(page, { origin: site, image, structuredData });
  const body = server.render(page.path);
  return {
    file: page.path === "/" ? "index.html" : `${page.path.slice(1)}.html`,
    html: fill(fill(html, "<title>flexdeck</title>", head), '<div id="root"></div>', `<div id="root">${body}</div>`),
  };
});
const notFound = fill(html, "<title>flexdeck</title>", '<title>Not found · flexdeck</title>\n    <meta name="robots" content="noindex" />');

const config = {
  version: 3,
  routes: [
    // The editor lives at the cloud's root, and its API beside it.
    { src: "^/config(/.*)?$", status: 308, headers: { Location: `${editor}/` } },
    { src: "^/api(/.*)?$", status: 308, headers: { Location: `${editor}/api$1` } },
    { src: "^/docs/?$", status: 308, headers: { Location: `/docs/${server.DOCS[0]!.slug}` } },
    { src: "^/(.+)/$", status: 308, headers: { Location: "/$1" } },
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
    ...pages.filter((page) => page.path !== "/").map((page) => ({ src: `^${page.path}$`, dest: `${page.path}.html` })),
    { src: "^/(.*)$", dest: "/404.html", status: 404 },
  ],
};

rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });
cpSync(dist, join(output, "static"), { recursive: true });
for (const { file, html: page } of prerendered) {
  mkdirSync(dirname(join(output, "static", file)), { recursive: true });
  writeFileSync(join(output, "static", file), page);
}
writeFileSync(join(output, "static", "404.html"), notFound);
writeFileSync(join(output, "static", "sitemap.xml"), sitemap(site, pages.map((page) => page.path)));
writeFileSync(join(output, "static", "robots.txt"), robots(site));
writeFileSync(join(output, "static", "llms.txt"), llmsTxt(site, pages));
writeFileSync(join(output, "config.json"), `${JSON.stringify(config, null, 2)}\n`);
console.log(
  `vercel-output: ${output} (editor at ${editor}, ${prerendered.length} prerendered page(s), ${hashes ? hashes.trim().split(" ").length : 0} inline script hash(es))`,
);
