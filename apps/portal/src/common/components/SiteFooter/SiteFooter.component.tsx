import { Mark, cn } from "@flexdeck/ui";

import { REPO_URL } from "../../release/release.lib.ts";
import { SITE_HREF } from "../../site/site.ts";
import { AppLink } from "../AppLink/AppLink.component.tsx";

const onSite = (path: string) => (SITE_HREF === "/" ? path : `${SITE_HREF}${path}`);

const LINKS = [
  { label: "Docs", to: onSite("/docs") },
  { label: "Changelog", to: onSite("/changelog") },
  { label: "Download", to: onSite("/download") },
  { label: "GitHub", to: REPO_URL },
  { label: "Issues", to: `${REPO_URL}/issues` },
];

const linkClass = "text-faint no-underline transition-colors hover:text-ink";

export function SiteFooter({ className }: { className?: string }) {
  return (
    <footer className={cn("border-t border-line/70", className)}>
      <div className="mx-auto flex max-w-[1440px] flex-wrap items-center justify-between gap-4 px-[clamp(16px,4vw,48px)] py-5 font-mono text-[10px] tracking-[0.18em] text-faint uppercase">
        <AppLink to={SITE_HREF} className="flex items-center gap-2.5 text-muted no-underline hover:text-ink">
          <Mark className="h-4" />
          FlexDeck · AGPL-3.0
        </AppLink>
        <nav aria-label="Footer" className="flex flex-wrap items-center gap-[clamp(14px,3vw,28px)]">
          {LINKS.map((link) =>
            link.to.startsWith(REPO_URL) ? (
              <a key={link.label} href={link.to} target="_blank" rel="noopener noreferrer" className={linkClass}>
                {link.label}
              </a>
            ) : (
              <AppLink key={link.label} to={link.to} className={linkClass}>
                {link.label}
              </AppLink>
            ),
          )}
        </nav>
      </div>
    </footer>
  );
}
