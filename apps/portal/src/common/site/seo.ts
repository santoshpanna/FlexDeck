export const SITE_NAME = "flexdeck";
export const SITE_ORIGIN = "https://flex-deck.com";

export const HOME_TITLE = "flexdeck · Free, open-source Stream Deck alternative for your tablet";

export function pageTitle(title?: string): string {
  return title ? `${title} · ${SITE_NAME}` : HOME_TITLE;
}

export interface PageMeta {
  path: string;
  title: string;
  description: string;
}

export interface DocSummary {
  slug: string;
  title: string;
  html: string;
}

const DOC_DESCRIPTIONS: Record<string, string> = {
  "getting-started":
    "Set up flexdeck in about five minutes: install the Windows app, pair your Android tablet with a six-digit code, and put your first actions on keys.",
  plugins:
    "How flexdeck plugins add actions, live variables and events. Install OBS Studio, Spotify, Discord and more from reviewed repositories, or write your own.",
  mcp: "Let Claude Code, Cursor or VS Code edit your flexdeck keys, pages and automations over MCP, with a revocable token that only works on this computer.",
  architecture:
    "How flexdeck works: the Windows desktop agent, the Android client, the web editor, and the contract that connects them over your local network.",
  security:
    "flexdeck's security model: HTTPS with a pinned certificate on your own Wi-Fi, six-digit pairing, no accounts, and plugin permissions shown up front.",
  development:
    "Build and test flexdeck from source: requirements, the monorepo layout, running the desktop, Android and web apps, and the release pipeline.",
};

const ENTITIES: Record<string, string> = { "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&#39;": "'" };

export function summarize(html: string, limit = 160): string {
  const withoutQuotes = html.replace(/<blockquote[\s\S]*?<\/blockquote>/g, "");
  const paragraph = /<p>([\s\S]*?)<\/p>/.exec(withoutQuotes)?.[1] ?? "";
  const text = paragraph
    .replace(/<[^>]+>/g, "")
    .replace(/&(amp|lt|gt|quot|#39);/g, (entity) => ENTITIES[entity] ?? entity)
    .replace(/\s+/g, " ")
    .trim();
  if (text.length <= limit) return text;
  const cut = text.slice(0, limit - 1);
  return `${cut.slice(0, Math.max(cut.lastIndexOf(" "), 0) || cut.length)}…`;
}

export function publicPages(docs: readonly DocSummary[]): PageMeta[] {
  return [
    {
      path: "/",
      title: HOME_TITLE,
      description:
        "Turn the Android tablet in your drawer into a Stream Deck-style control surface for your Windows PC. Keys, faders, live states and automations for OBS, Spotify, Discord and more. Free, open source, no accounts.",
    },
    {
      path: "/download",
      title: pageTitle("Download"),
      description:
        "Download flexdeck for Windows 10/11 and Android 5.0+. Free and open source: install the desktop app, pair your tablet with a six-digit code, and build your deck.",
    },
    {
      path: "/changelog",
      title: pageTitle("Changelog"),
      description: "Every flexdeck release: new features and fixes for the Windows desktop app, the Android app, the web editor and plugins.",
    },
    ...docs.map((doc) => ({
      path: `/docs/${doc.slug}`,
      title: pageTitle(doc.title),
      description: DOC_DESCRIPTIONS[doc.slug] ?? summarize(doc.html),
    })),
  ];
}

const escapeHtml = (value: string) =>
  value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

const jsonLd = (data: unknown) => JSON.stringify(data).replace(/</g, "\\u003c");

export interface HeadOptions {
  origin: string;
  image: string;
  structuredData?: unknown[];
}

export function canonicalUrl(origin: string, path: string): string {
  return path === "/" ? `${origin}/` : `${origin}${path}`;
}

export function headTags(page: PageMeta, { origin, image, structuredData = [] }: HeadOptions): string {
  const url = canonicalUrl(origin, page.path);
  const title = escapeHtml(page.title);
  const description = escapeHtml(page.description);
  return [
    `<title>${title}</title>`,
    `<meta name="description" content="${description}" />`,
    `<link rel="canonical" href="${escapeHtml(url)}" />`,
    `<meta property="og:type" content="${page.path === "/" ? "website" : "article"}" />`,
    `<meta property="og:site_name" content="${SITE_NAME}" />`,
    `<meta property="og:title" content="${title}" />`,
    `<meta property="og:description" content="${description}" />`,
    `<meta property="og:url" content="${escapeHtml(url)}" />`,
    `<meta property="og:image" content="${escapeHtml(image)}" />`,
    `<meta name="twitter:card" content="summary" />`,
    `<meta name="twitter:title" content="${title}" />`,
    `<meta name="twitter:description" content="${description}" />`,
    `<meta name="twitter:image" content="${escapeHtml(image)}" />`,
    ...structuredData.map((data) => `<script type="application/ld+json">${jsonLd(data)}</script>`),
  ].join("\n    ");
}

export interface AppFacts {
  origin: string;
  image: string;
  repository: string;
  downloadUrl: string;
  version: string | null;
  description: string;
}

export function softwareApplication(facts: AppFacts): object {
  return {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    name: SITE_NAME,
    description: facts.description,
    url: `${facts.origin}/`,
    image: facts.image,
    applicationCategory: "UtilitiesApplication",
    operatingSystem: "Windows 10, Windows 11, Android 5.0+",
    downloadUrl: facts.downloadUrl,
    ...(facts.version ? { softwareVersion: facts.version.replace(/^v/, "") } : {}),
    license: "https://www.gnu.org/licenses/agpl-3.0.html",
    isAccessibleForFree: true,
    offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
    sameAs: [facts.repository],
  };
}

export function website(origin: string): object {
  return { "@context": "https://schema.org", "@type": "WebSite", name: SITE_NAME, url: `${origin}/` };
}

export function breadcrumbs(origin: string, trail: readonly { name: string; path: string }[]): object {
  return {
    "@context": "https://schema.org",
    "@type": "BreadcrumbList",
    itemListElement: trail.map((crumb, index) => ({
      "@type": "ListItem",
      position: index + 1,
      name: crumb.name,
      item: canonicalUrl(origin, crumb.path),
    })),
  };
}

export function sitemap(origin: string, paths: readonly string[], lastModified?: string): string {
  const entries = paths.map(
    (path) =>
      `  <url><loc>${escapeHtml(canonicalUrl(origin, path))}</loc>${lastModified ? `<lastmod>${lastModified}</lastmod>` : ""}</url>`,
  );
  return `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${entries.join("\n")}\n</urlset>\n`;
}

export function robots(origin: string): string {
  return `User-agent: *\nAllow: /\nDisallow: /config\nDisallow: /api\n\nSitemap: ${origin}/sitemap.xml\n`;
}

export function llmsTxt(origin: string, pages: readonly PageMeta[]): string {
  const home = pages.find((page) => page.path === "/");
  const link = (page: PageMeta) => `- [${page.title.split(" · ")[0]}](${canonicalUrl(origin, page.path)}): ${page.description}`;
  const section = (heading: string, list: readonly PageMeta[]) => (list.length ? [`## ${heading}`, "", ...list.map(link), ""] : []);
  const docs = pages.filter((page) => page.path.startsWith("/docs/"));
  const optional = pages.filter((page) => page.path === "/changelog");
  const rest = pages.filter((page) => page !== home && !docs.includes(page) && !optional.includes(page));
  return [
    `# ${SITE_NAME}`,
    "",
    ...(home ? [`> ${home.description}`, ""] : []),
    ...section("Docs", docs),
    ...section("Pages", rest),
    ...section("Optional", optional),
  ].join("\n");
}
