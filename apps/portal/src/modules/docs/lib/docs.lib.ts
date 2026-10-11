import { Marked } from "marked";

import { REPO_URL } from "../../../common/release/release.lib.ts";

/**
 * The docs site, rendered from the same `docs/*.md` files GitHub shows.
 *
 * One source, two readers: the files stay plain GitHub Markdown, and this turns
 * them into pages — heading ids that match GitHub's (so `plugins.md#the-protocol`
 * keeps working), links between docs rewritten to routes, links out of `docs/`
 * pointed at the repository, and an outline for the "on this page" rail.
 *
 * Pure: `docs.content.ts` supplies the files.
 */

export interface DocHeading {
  id: string;
  text: string;
  depth: 2 | 3;
}

export interface Doc {
  slug: string;
  title: string;
  html: string;
  headings: DocHeading[];
}

export interface NavItem {
  label: string;
  /** A route inside the site, or an absolute URL. */
  to: string;
}

export interface NavGroup {
  label: string;
  items: NavItem[];
}

/** Reading order: the sidebar, and each page's next link. */
export const DOC_ORDER = ["getting-started", "plugins", "architecture", "security", "development"] as const;

/** The sidebar, grouped the way `docs/README.md` groups the docs. */
export const NAV: NavGroup[] = [
  {
    label: "Using flexdeck",
    items: [
      { label: "Getting started", to: "/docs/getting-started" },
      { label: "Plugins", to: "/docs/plugins" },
    ],
  },
  {
    label: "Building on flexdeck",
    items: [
      { label: "Writing a plugin", to: "/docs/plugins#writing-a-plugin" },
      { label: "The plugin protocol", to: "/docs/plugins#the-protocol" },
      { label: "Recipes, no code", to: "/docs/plugins#recipes-plugins-without-code" },
    ],
  },
  {
    label: "Working on flexdeck",
    items: [
      { label: "Architecture", to: "/docs/architecture" },
      { label: "Security", to: "/docs/security" },
      { label: "Development", to: "/docs/development" },
      { label: "Contributing", to: `${REPO_URL}/blob/main/CONTRIBUTING.md` },
    ],
  },
];

/** Inline Markdown → the words a reader sees: `code`, **bold**, [links](x) and tags stripped. */
export function plainText(markdown: string): string {
  return markdown
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/<[^>]+>/g, "")
    .replace(/[`*_]/g, "")
    .trim();
}

/** GitHub's heading anchor: lower case, punctuation dropped (but `-` and `_`), each space a `-`. */
export function slugify(text: string): string {
  return plainText(text)
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .replace(/\s/g, "-");
}

/**
 * A link as written in a doc → where it should go on the site.
 *
 *   plugins.md#x      → /docs/plugins#x       (another doc)
 *   README.md         → /docs                 (the docs index)
 *   ../SECURITY.md    → the repository        (outside docs/)
 *   examples/a.yml    → the repository        (a file in docs/ that is not a page)
 */
export function rewriteHref(href: string, slugs: readonly string[]): string {
  if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("#") || href.startsWith("/")) return href;
  const [path, hash = ""] = href.split("#", 2);
  const anchor = hash ? `#${hash}` : "";
  if (path === "README.md") return `/docs${anchor}`;
  const page = /^([\w-]+)\.md$/.exec(path);
  if (page && slugs.includes(page[1])) return `/docs/${page[1]}${anchor}`;
  if (path.startsWith("../")) return `${REPO_URL}/blob/main/${path.slice(3)}${anchor}`;
  return `${REPO_URL}/blob/main/docs/${path}${anchor}`;
}

const escapeAttribute = (value: string) => value.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");

/** "Part of the flexdeck docs." heads each file for GitHub readers; on the site the sidebar says it. */
function stripBreadcrumb(markdown: string): string {
  // The breadcrumb opens a quote; drop the sentence, and the quote's line if nothing else was on it.
  return markdown.replace(/^>[ \t]*Part of the \[flexdeck docs\]\(README\.md\)\.[ \t]*(.*)\r?\n/m, (_, rest: string) => (rest.trim() ? `> ${rest.trim()}\n` : ""));
}

/** One Markdown file → a page: its title, its body as HTML (minus the title), and its outline. */
export function renderDoc(slug: string, markdown: string, slugs: readonly string[]): Doc {
  const source = stripBreadcrumb(markdown);
  const titleMatch = /^#[ \t]+(.+)$/m.exec(source);
  const title = titleMatch ? plainText(titleMatch[1]) : slug;
  const body = titleMatch ? source.replace(titleMatch[0], "") : source;

  const headings: DocHeading[] = [];
  const used = new Map<string, number>();
  const unique = (base: string) => {
    const count = used.get(base) ?? 0;
    used.set(base, count + 1);
    return count === 0 ? base : `${base}-${count}`;
  };

  const marked = new Marked({
    gfm: true,
    renderer: {
      heading({ tokens, depth, text }) {
        const id = unique(slugify(text));
        if (depth === 2 || depth === 3) headings.push({ id, text: plainText(text), depth });
        return `<h${depth} id="${id}">${this.parser.parseInline(tokens)}</h${depth}>\n`;
      },
      link({ href, title: linkTitle, tokens }) {
        const target = rewriteHref(href, slugs);
        const external = /^https?:/i.test(target);
        const titleAttribute = linkTitle ? ` title="${escapeAttribute(linkTitle)}"` : "";
        const rel = external ? ` target="_blank" rel="noreferrer"` : "";
        return `<a href="${escapeAttribute(target)}"${titleAttribute}${rel}>${this.parser.parseInline(tokens)}</a>`;
      },
    },
  });

  return { slug, title, html: marked.parse(body, { async: false }), headings };
}

export interface SearchHit {
  to: string;
  doc: string;
  heading: string | null;
}

/** Titles and headings matching every word of `query`, page titles first. At most `limit`. */
export function searchDocs(docs: readonly Doc[], query: string, limit = 8): SearchHit[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [];
  const matches = (text: string) => words.every((word) => text.toLowerCase().includes(word));
  const pages = docs.filter((doc) => matches(doc.title)).map((doc) => ({ to: `/docs/${doc.slug}`, doc: doc.title, heading: null }));
  const sections = docs.flatMap((doc) =>
    doc.headings.filter((heading) => matches(`${doc.title} ${heading.text}`)).map((heading) => ({ to: `/docs/${doc.slug}#${heading.id}`, doc: doc.title, heading: heading.text })),
  );
  return [...pages, ...sections].slice(0, limit);
}

/** The doc after this one in reading order, or null at the end. */
export function nextDoc<T extends { slug: string }>(docs: readonly T[], slug: string): T | null {
  const index = docs.findIndex((doc) => doc.slug === slug);
  return index >= 0 && index < docs.length - 1 ? docs[index + 1] : null;
}
