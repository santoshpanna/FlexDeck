import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { ASSETS, REPO_URL, detectPlatform, latestRelease, type Release } from "../src/common/release/release.lib.ts";
import { countChanges, filterGroups } from "../src/modules/changelog/lib/changelog.lib.ts";
import { DOC_ORDER, nextDoc, renderDoc, rewriteHref, searchDocs, slugify } from "../src/modules/docs/lib/docs.lib.ts";

/**
 * The public pages around the editor: downloads, the changelog and the docs.
 * The docs tests render the real files in `docs/`, so a broken link between
 * them fails here, not on the site.
 */

const readDoc = (slug: string) => readFileSync(new URL(`../../../docs/${slug}.md`, import.meta.url), "utf8");

test("downloads point at the newest release's file of a fixed name", () => {
  assert.equal(ASSETS.windows.url, `${REPO_URL}/releases/latest/download/flexdeck-windows-x64-setup.exe`);
  assert.equal(ASSETS.android.url, `${REPO_URL}/releases/latest/download/flexdeck-android.apk`);
});

test("the platform guess reads Android before Windows", () => {
  assert.equal(detectPlatform("Mozilla/5.0 (Linux; Android 5.1; Nexus 7)"), "android");
  assert.equal(detectPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"), "windows");
  assert.equal(detectPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)"), "other");
});

test("the latest release is the first one, or none yet", () => {
  assert.equal(latestRelease({ next: [], releases: [] }), null);
  const release: Release = { version: "v0.1.0", platforms: ["desktop", "android"], hash: "a", date: "2026-10-20", title: "", blurb: "", groups: [] };
  assert.equal(latestRelease({ next: [], releases: [release] }), release);
});

test("filtering the changelog by part drops emptied groups", () => {
  const groups = [
    { name: "Added" as const, items: [{ area: "ANDROID" as const, text: "Faders." }, { area: "DESKTOP" as const, text: "Pairing." }] },
    { name: "Fixed" as const, items: [{ area: "DESKTOP" as const, text: "Tray." }] },
  ];
  assert.equal(filterGroups(groups, "ALL"), groups);
  assert.deepEqual(filterGroups(groups, "ANDROID"), [{ name: "Added", items: [{ area: "ANDROID", text: "Faders." }] }]);
  assert.deepEqual(filterGroups(groups, "PLUGINS"), []);
  assert.equal(countChanges(groups), 3);
});

test("heading anchors match GitHub's", () => {
  assert.equal(slugify("Recipes: plugins without code"), "recipes-plugins-without-code");
  assert.equal(slugify("ABI 4: capabilities, feeds, presets, series"), "abi-4-capabilities-feeds-presets-series");
  assert.equal(slugify("The `parity` problem"), "the-parity-problem");
});

test("links between docs become routes; links out of docs go to the repository", () => {
  assert.equal(rewriteHref("plugins.md", DOC_ORDER), "/docs/plugins");
  assert.equal(rewriteHref("plugins.md#the-protocol", DOC_ORDER), "/docs/plugins#the-protocol");
  assert.equal(rewriteHref("README.md", DOC_ORDER), "/docs");
  assert.equal(rewriteHref("../SECURITY.md", DOC_ORDER), `${REPO_URL}/blob/main/SECURITY.md`);
  assert.equal(rewriteHref("examples/plugin-repo.yml", DOC_ORDER), `${REPO_URL}/blob/main/docs/examples/plugin-repo.yml`);
  assert.equal(rewriteHref("https://flex-deck.com", DOC_ORDER), "https://flex-deck.com");
  assert.equal(rewriteHref("#troubleshooting", DOC_ORDER), "#troubleshooting");
});

test("a doc renders with its title lifted out and the breadcrumb gone", () => {
  const doc = renderDoc("getting-started", readDoc("getting-started"), DOC_ORDER);
  assert.equal(doc.title, "Getting started");
  assert.doesNotMatch(doc.html, /<h1/);
  assert.doesNotMatch(doc.html, /Part of the/);
  assert.match(doc.html, /<h2 id="1-install-the-desktop-app">/);
  assert.match(doc.html, /href="\/docs\/plugins"/);
  assert.deepEqual(
    doc.headings.map((heading) => heading.id),
    ["1-install-the-desktop-app", "2-install-the-android-app", "3-pair-the-tablet", "4-build-your-deck", "5-add-plugins", "troubleshooting"],
  );
});

test("a breadcrumb sharing its quote keeps the rest of the quote", () => {
  const doc = renderDoc("security", readDoc("security"), DOC_ORDER);
  assert.doesNotMatch(doc.html, /Part of the/);
  assert.match(doc.html, /<blockquote>[\s\S]*Found a vulnerability/);
  assert.match(doc.html, new RegExp(`href="${REPO_URL}/blob/main/SECURITY.md"`));
});

test("every link between the real docs lands on a heading that exists", () => {
  const docs = DOC_ORDER.map((slug) => renderDoc(slug, readDoc(slug), DOC_ORDER));
  const ids = new Map(docs.map((doc) => [doc.slug, new Set(doc.headings.map((heading) => heading.id))]));
  for (const doc of docs) {
    for (const [, slug, hash] of doc.html.matchAll(/href="\/docs\/([\w-]+)#([^"]+)"/g)) {
      assert.ok(ids.get(slug)?.has(hash), `${doc.slug} links to /docs/${slug}#${hash}, which is not a heading`);
    }
  }
});

test("search finds pages by title and sections by heading", () => {
  const docs = DOC_ORDER.map((slug) => renderDoc(slug, readDoc(slug), DOC_ORDER));
  assert.deepEqual(searchDocs(docs, "plugins")[0], { to: "/docs/plugins", doc: "Plugins", heading: null });
  assert.ok(searchDocs(docs, "pair tablet").some((hit) => hit.to === "/docs/getting-started#3-pair-the-tablet"));
  assert.deepEqual(searchDocs(docs, "   "), []);
  assert.equal(nextDoc(docs, "getting-started")?.slug, "plugins");
  assert.equal(nextDoc(docs, "development"), null);
});
