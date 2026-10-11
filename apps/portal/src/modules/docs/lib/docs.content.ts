import { DOC_ORDER, type Doc, renderDoc } from "./docs.lib.ts";

/**
 * The repository's `docs/*.md`, bundled at build time and rendered once.
 *
 * `eager` and `?raw`: the Markdown ships inside the bundle as strings, so the
 * pages need no request and the site cannot drift from the files on GitHub.
 */
const files = import.meta.glob<string>("../../../../../../docs/*.md", { query: "?raw", import: "default", eager: true });

const sources = new Map(Object.entries(files).map(([path, text]) => [path.split("/").pop()!.replace(/\.md$/, ""), text]));

/** Every doc page, in reading order. Files not in `DOC_ORDER` (the README) are not pages. */
export const DOCS: Doc[] = DOC_ORDER.filter((slug) => sources.has(slug)).map((slug) => renderDoc(slug, sources.get(slug)!, DOC_ORDER));
