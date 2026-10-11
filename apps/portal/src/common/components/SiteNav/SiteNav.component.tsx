import type { ReactNode } from "react";
import { Link, NavLink } from "react-router-dom";

import { Mark, cn, keyVariants } from "@flexdeck/ui";

import { EDITOR_HREF } from "../../site/site.ts";
import { AppLink } from "../AppLink/AppLink.component.tsx";
import { ThemeToggle } from "../ThemeToggle/ThemeToggle.component.tsx";

export interface SiteNavAnchor {
  label: string;
  href: string;
}

const SECTIONS = [
  { label: "Docs", to: "/docs" },
  { label: "Changelog", to: "/changelog" },
  { label: "Download", to: "/download" },
] as const;

const linkClass = "font-mono text-[11px] tracking-[0.18em] uppercase no-underline transition-colors";

/**
 * The bar across the top of every public page: the mark home, in-page anchors
 * when a page has them, the site's sections, appearance, and the editor.
 *
 * `children` paint inside the bar — the landing page's scroll progress line.
 * `trail` sits after the wordmark, for a crumb like "/ docs".
 */
export function SiteNav({ anchors = [], trail, children }: { anchors?: SiteNavAnchor[]; trail?: ReactNode; children?: ReactNode }) {
  return (
    <nav aria-label="Main" className="sticky top-0 z-40 border-b border-line/70 bg-body/75 backdrop-blur-md">
      {children}
      <div className="mx-auto flex max-w-[1440px] flex-wrap items-center justify-between gap-4 px-[clamp(16px,4vw,48px)] py-4">
        <div className="flex items-center gap-4">
          <Link to="/" className="flex items-center gap-3 text-base font-bold tracking-[0.34em] text-ink no-underline">
            <Mark className="h-6" />
            FLEXDECK
          </Link>
          {trail}
        </div>
        <div className="flex flex-wrap items-center gap-[clamp(14px,3vw,32px)]">
          {anchors.map((anchor) => (
            <a key={anchor.href} href={anchor.href} className={cn(linkClass, "text-muted hover:text-ink")}>
              {anchor.label}
            </a>
          ))}
          {SECTIONS.map((section) => (
            <NavLink key={section.to} to={section.to} className={({ isActive }) => cn(linkClass, isActive ? "text-ink" : "text-muted hover:text-ink")}>
              {section.label}
            </NavLink>
          ))}
          <ThemeToggle />
          <AppLink to={EDITOR_HREF} className={cn(keyVariants({ tone: "signal" }), "text-[13px] no-underline")}>
            Open the editor
          </AppLink>
        </div>
      </div>
    </nav>
  );
}
