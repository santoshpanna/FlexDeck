import { motion } from "motion/react";
import {
  type MouseEvent,
  type RefObject,
  useEffect,
  useRef,
  useState,
} from "react";
import { Link, useLocation, useNavigate } from "react-router-dom";

import { cn, ease, keyVariants } from "@flexdeck/ui";

import { SiteNav } from "../../../common/components/SiteNav/SiteNav.component.tsx";
import { REPO_URL } from "../../../common/release/release.lib.ts";
import { useDocumentTitle } from "../../../hooks/useDocumentTitle/useDocumentTitle.hook.ts";
import { DocsSearch } from "../components/DocsSearch/DocsSearch.component.tsx";
import type { Doc, NavGroup } from "../lib/docs.lib.ts";
import "./docs.css";

export interface DocsViewProps {
  doc: Doc;
  docs: Doc[];
  nav: NavGroup[];
  next: Doc | null;
}

/**
 * The heading the reader is on, for the outline. Watched, not polled: an
 * IntersectionObserver reports headings crossing a band near the top.
 */
function useActiveHeading(root: RefObject<HTMLElement | null>, key: string) {
  const [active, setActive] = useState<string | null>(null);
  useEffect(() => {
    const element = root.current;
    if (!element) return;
    setActive(null);
    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((entry) => entry.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
        if (visible[0]) setActive(visible[0].target.id);
      },
      { rootMargin: "-96px 0px -65% 0px" },
    );
    element
      .querySelectorAll("h2[id], h3[id]")
      .forEach((heading) => observer.observe(heading));
    return () => observer.disconnect();
  }, [root, key]);
  return active;
}

/**
 * A docs page: the sections on the left, the doc in the middle, its outline on
 * the right. On a phone the three stack.
 */
export function DocsView(props: Partial<DocsViewProps>) {
  const { doc, docs = [], nav = [], next = null } = props;
  const navigate = useNavigate();
  const location = useLocation();
  const article = useRef<HTMLDivElement>(null);
  const active = useActiveHeading(article, doc?.slug ?? "");
  const [sectionsOpen, setSectionsOpen] = useState(false);
  useDocumentTitle(doc?.title);

  // A new page starts at the top; a link to a section lands on it.
  useEffect(() => {
    setSectionsOpen(false);
    const id = decodeURIComponent(location.hash.slice(1));
    const target = id ? document.getElementById(id) : null;
    if (target) target.scrollIntoView();
    else window.scrollTo(0, 0);
  }, [doc?.slug, location.hash]);

  if (!doc) return null;
  const here = `/docs/${doc.slug}`;

  // Links between docs are plain <a> in rendered Markdown; route them without a reload.
  const followLink = (event: MouseEvent<HTMLDivElement>) => {
    const href = (event.target as Element).closest("a")?.getAttribute("href");
    if (
      !href?.startsWith("/") ||
      event.button !== 0 ||
      event.metaKey ||
      event.ctrlKey ||
      event.shiftKey ||
      event.altKey
    )
      return;
    event.preventDefault();
    navigate(href);
  };

  return (
    <div className="min-h-screen bg-body text-ink">
      <SiteNav
        trail={
          <span className="font-mono text-[10px] tracking-[0.16em] text-faint uppercase">
            / Docs
          </span>
        }
      />
      <div className="mx-auto flex max-w-[1440px] flex-wrap items-start gap-14 px-[clamp(16px,4vw,48px)] pt-12 pb-32">
        <aside
          aria-label="Docs sections"
          className="flex flex-[1_1_240px] flex-col gap-7 min-[900px]:max-w-[260px]"
        >
          <div className="flex gap-2">
            <div className="min-w-0 flex-1">
              <DocsSearch docs={docs} />
            </div>
            {/* On a phone the sections fold away, so the doc is not pushed below fourteen links. */}
            <button
              type="button"
              aria-expanded={sectionsOpen}
              aria-controls="docs-sections"
              onClick={() => setSectionsOpen((open) => !open)}
              className={cn(keyVariants(), "min-[900px]:hidden")}
            >
              Sections
            </button>
          </div>
          <div
            id="docs-sections"
            className={cn(
              "flex flex-col gap-7",
              !sectionsOpen && "max-[899px]:hidden",
            )}
          >
            {nav.map((group) => (
              <div key={group.label} className="flex flex-col gap-1">
                <span className="lbl px-3 pb-2">{group.label}</span>
                {group.items.map((item) => {
                  const current = item.to === here;
                  const className = cn(
                    "block rounded-[10px] border px-3 py-2 text-sm no-underline transition-colors",
                    current
                      ? "border-line bg-plate text-ink"
                      : "border-transparent text-ink-2 hover:bg-plate hover:text-ink",
                  );
                  if (/^https?:/.test(item.to)) {
                    return (
                      <a
                        key={item.to}
                        href={item.to}
                        target="_blank"
                        rel="noreferrer"
                        className={className}
                      >
                        {item.label} ↗
                      </a>
                    );
                  }
                  return (
                    <Link
                      key={item.to}
                      to={item.to}
                      aria-current={current ? "page" : undefined}
                      className={className}
                    >
                      {current ? (
                        <span
                          aria-hidden="true"
                          className="ascii-glow mr-2 font-mono"
                        >
                          ›
                        </span>
                      ) : null}
                      {item.label}
                    </Link>
                  );
                })}
              </div>
            ))}
          </div>
        </aside>

        <motion.article
          key={doc.slug}
          initial={{ opacity: 0, y: 14 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5, ease: ease.outExpo }}
          className="max-w-[760px] min-w-0 flex-[999_1_560px]"
        >
          <span className="font-mono text-[10px] tracking-[0.18em] text-faint uppercase">
            Docs / <span className="text-muted">{doc.title}</span>
          </span>
          <h1 className="m-0 mt-4 font-mono text-[clamp(34px,4.4vw,64px)] leading-[0.98] font-medium tracking-[-0.03em]">
            {doc.title}
          </h1>
          {/* Our own repository's docs, rendered at build time — not user input. */}
          <div
            ref={article}
            className="doc mt-6"
            onClick={followLink}
            dangerouslySetInnerHTML={{ __html: doc.html }}
          />

          <div className="mt-16 flex justify-end border-t border-line pt-6">
            {next ? (
              <Link
                to={`/docs/${next.slug}`}
                className={cn(
                  keyVariants(),
                  "h-auto flex-col items-end gap-1 px-5 py-4 no-underline",
                )}
              >
                <span className="lbl">Next</span>
                <span className="text-base font-semibold">{next.title} →</span>
              </Link>
            ) : null}
          </div>
          <div className="mt-6 flex flex-wrap justify-between gap-3 font-mono text-[10px] tracking-[0.16em] text-faint uppercase">
            <a
              href={`${REPO_URL}/edit/main/docs/${doc.slug}.md`}
              target="_blank"
              rel="noreferrer"
              className="text-muted no-underline hover:text-ink"
            >
              Edit this page on GitHub
            </a>
            <Link
              to="/changelog"
              className="text-muted no-underline hover:text-ink"
            >
              Changelog
            </Link>
          </div>
        </motion.article>

        {doc.headings.length > 0 ? (
          <nav
            aria-label="On this page"
            className="sticky top-24 flex max-h-[calc(100vh-120px)] max-w-[220px] flex-[1_1_200px] flex-col gap-0.5 overflow-y-auto max-[1100px]:hidden"
          >
            <span className="lbl pb-3">On this page</span>
            {doc.headings.map((heading) => (
              <a
                key={heading.id}
                href={`#${heading.id}`}
                className={cn(
                  "border-l py-1.5 text-[13px] no-underline transition-colors",
                  heading.depth === 3 ? "pl-6" : "pl-3.5",
                  heading.id === active
                    ? "border-signal text-ink"
                    : "border-line text-muted hover:text-ink",
                )}
              >
                {heading.text}
              </a>
            ))}
          </nav>
        ) : null}
      </div>
    </div>
  );
}
