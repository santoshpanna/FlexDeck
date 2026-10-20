import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { DOC_ORDER, renderDoc } from "../src/modules/docs/lib/docs.lib.ts";
import {
  HOME_TITLE,
  breadcrumbs,
  canonicalUrl,
  headTags,
  pageTitle,
  publicPages,
  robots,
  sitemap,
  summarize,
} from "../src/common/site/seo.ts";

const docs = DOC_ORDER.map((slug) =>
  renderDoc(slug, readFileSync(new URL(`../../../docs/${slug}.md`, import.meta.url), "utf8"), DOC_ORDER),
);

test("page titles name the page, and the home page names what flexdeck is", () => {
  assert.equal(pageTitle("Download"), "Download · flexdeck");
  assert.equal(pageTitle(), HOME_TITLE);
});

test("every public page has a unique title and a search-sized description", () => {
  const pages = publicPages(docs);
  assert.deepEqual(
    pages.map((page) => page.path),
    ["/", "/download", "/changelog", ...DOC_ORDER.map((slug) => `/docs/${slug}`)],
  );
  assert.equal(new Set(pages.map((page) => page.title)).size, pages.length);
  for (const page of pages) {
    assert.ok(page.description.length >= 50, `${page.path} description too short`);
    assert.ok(page.description.length <= 230, `${page.path} description too long`);
  }
});

test("a doc without a written description is summarized from its first paragraph", () => {
  const html = "<blockquote><p>Part of the docs.</p></blockquote><p>Hello <code>x</code> &amp; <b>more</b>.</p>";
  assert.equal(summarize(html), "Hello x & more.");
  const long = summarize(`<p>${"word ".repeat(80)}</p>`, 40);
  assert.ok(long.length <= 40);
  assert.ok(long.endsWith("…"));
  assert.equal(publicPages([{ slug: "unlisted", title: "Unlisted", html }]).at(-1)?.description, "Hello x & more.");
});

test("head tags escape page text and keep structured data inert", () => {
  const head = headTags(
    { path: "/docs/x", title: 'A "quoted" <title>', description: "a & b" },
    { origin: "https://example.com", image: "https://example.com/i.png", structuredData: [{ name: "</script><script>" }] },
  );
  assert.match(head, /<title>A &quot;quoted&quot; &lt;title&gt;<\/title>/);
  assert.match(head, /content="a &amp; b"/);
  assert.match(head, /<link rel="canonical" href="https:\/\/example.com\/docs\/x" \/>/);
  assert.ok(!head.includes("</script><script>"));
  assert.match(head, /\\u003c\/script>/);
});

test("canonical URLs, sitemap and robots point at the public origin", () => {
  assert.equal(canonicalUrl("https://example.com", "/"), "https://example.com/");
  const xml = sitemap("https://example.com", ["/", "/download"]);
  assert.match(xml, /<loc>https:\/\/example.com\/<\/loc>/);
  assert.match(xml, /<loc>https:\/\/example.com\/download<\/loc>/);
  const text = robots("https://example.com");
  assert.match(text, /Disallow: \/config/);
  assert.match(text, /Sitemap: https:\/\/example.com\/sitemap.xml/);
  const crumbs = breadcrumbs("https://example.com", [{ name: "flexdeck", path: "/" }, { name: "Plugins", path: "/docs/plugins" }]) as {
    itemListElement: { position: number; item: string }[];
  };
  assert.deepEqual(
    crumbs.itemListElement.map((entry) => [entry.position, entry.item]),
    [
      [1, "https://example.com/"],
      [2, "https://example.com/docs/plugins"],
    ],
  );
});
