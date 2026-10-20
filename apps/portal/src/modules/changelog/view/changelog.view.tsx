import { motion } from "motion/react";
import { type ReactNode, useState } from "react";
import { Link } from "react-router-dom";

import { Plate, Segmented, cn, ease, keyVariants } from "@flexdeck/ui";

import { SiteNav } from "../../../common/components/SiteNav/SiteNav.component.tsx";
import { SiteFooter } from "../../../common/components/SiteFooter/SiteFooter.component.tsx";
import { REPO_URL, type Release, type ReleaseGroup } from "../../../common/release/release.lib.ts";
import { useDocumentTitle } from "../../../hooks/useDocumentTitle/useDocumentTitle.hook.ts";
import { AREA_FILTERS, type AreaFilter, filterGroups } from "../lib/changelog.lib.ts";

export interface ChangelogViewProps {
  next: ReleaseGroup[];
  releases: Release[];
}

function Groups({ groups, label }: { groups: ReleaseGroup[]; label: string }) {
  if (groups.length === 0) {
    return <p className="m-0 font-mono text-xs tracking-[0.1em] text-faint uppercase">Nothing for this part in {label}.</p>;
  }
  return (
    <>
      {groups.map((group) => (
        <div key={group.name} className="flex flex-col gap-2.5">
          <span className="font-mono text-[10px] tracking-[0.2em] text-signal uppercase">{group.name}</span>
          <ul className="m-0 flex list-none flex-col p-0">
            {group.items.map((item) => (
              <li key={`${item.area}|${item.text}`} className="flex gap-4 border-b border-dashed border-line py-2.5 text-[15px] leading-relaxed text-ink-2">
                <span className="min-w-[72px] pt-[3px] font-mono text-[10px] tracking-[0.14em] text-muted">{item.area === "GENERAL" ? "" : item.area}</span>
                <span className="min-w-0 flex-1">{item.text}</span>
              </li>
            ))}
          </ul>
        </div>
      ))}
    </>
  );
}

/** One release down the page: the version as an outlined numeral on the left, what changed on the right. */
function Entry({ version, aside, children }: { version: string; aside: ReactNode; children: ReactNode }) {
  return (
    <motion.section
      aria-label={version}
      initial={{ opacity: 0, y: 30 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: "-60px" }}
      transition={{ duration: 0.7, ease: ease.outExpo }}
      className="flex flex-wrap gap-8 border-t border-line py-10"
    >
      <div className="flex max-w-[260px] flex-[1_1_220px] flex-col gap-2.5">
        <span className="font-mono text-[36px] leading-none font-medium tracking-[-0.04em] break-all text-transparent [-webkit-text-stroke:1px_var(--color-line-strong)]">{version}</span>
        {aside}
      </div>
      <div className="flex min-w-0 flex-[999_1_520px] flex-col gap-6">{children}</div>
    </motion.section>
  );
}

/**
 * The changelog: what is on main but unreleased, then every release, newest
 * first, filterable by part of the product. Generated from commits — nothing
 * here is written by hand.
 */
export function ChangelogView(props: Partial<ChangelogViewProps>) {
  const { next = [], releases = [] } = props;
  const [area, setArea] = useState<AreaFilter>("ALL");
  useDocumentTitle("Changelog");
  const pending = filterGroups(next, area);

  return (
    <div className="min-h-screen bg-body text-ink">
      <SiteNav />
      <main className="mx-auto flex max-w-[1200px] flex-col gap-12 px-[clamp(16px,4vw,48px)] pt-20 pb-32">
        <motion.header
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.8, ease: ease.outExpo }}
          className="flex flex-wrap items-end justify-between gap-8"
        >
          <div className="flex max-w-[640px] flex-col gap-6">
            <span className="lbl text-signal">// changelog</span>
            <h1 className="m-0 font-mono text-[clamp(36px,5.4vw,82px)] leading-[0.98] font-medium tracking-[-0.03em]">What changed.</h1>
            <p className="m-0 text-lg leading-relaxed text-ink-2">Every release of the desktop app, the tablet app and the editor. The editor updates itself; the two apps tell you when there’s a new version.</p>
          </div>
          <a href={`${REPO_URL}/releases`} className={cn(keyVariants(), "no-underline")}>
            GitHub releases
          </a>
        </motion.header>

        <Segmented label="Filter by part" value={area} onChange={setArea} options={AREA_FILTERS} className="self-start" />

        <div className="flex flex-col">
          {next.length > 0 ? (
            <Entry version="next" aside={<span className="font-mono text-[10px] tracking-[0.2em] text-faint uppercase">On main · not released</span>}>
              <Groups groups={pending} label="what’s next" />
            </Entry>
          ) : null}

          {releases.map((release, index) => (
            <Entry
              key={release.version}
              version={release.version}
              aside={
                <>
                  <span className="font-mono text-[11px] tracking-[0.14em] text-muted">{release.date}</span>
                  {index === 0 ? (
                    <>
                      <span className="flex items-center gap-2.5 font-mono text-[10px] tracking-[0.2em] text-ink uppercase">
                        <span aria-hidden="true" className="led-glow size-1.5 rounded-full bg-signal text-signal" />
                        Latest
                      </span>
                      <Link to="/download" className={cn(keyVariants({ tone: "signal" }), "mt-1.5 self-start text-[13px] no-underline")}>
                        Download
                      </Link>
                    </>
                  ) : null}
                </>
              }
            >
              {release.title ? (
                <div className="flex flex-col gap-2">
                  <h2 className="m-0 text-[26px] font-semibold tracking-[-0.02em]">{release.title}</h2>
                  {release.blurb ? <p className="m-0 text-base leading-relaxed text-ink-2">{release.blurb}</p> : null}
                </div>
              ) : null}
              <Groups groups={filterGroups(release.groups, area)} label={release.version} />
            </Entry>
          ))}

          {next.length === 0 && releases.length === 0 ? <p className="m-0 border-t border-line py-10 text-muted">Nothing yet. The first changes land with the first release.</p> : null}
        </div>

        <Plate pad="none" className="flex flex-wrap items-center justify-between gap-4 p-6">
          <p className="m-0 text-[15px] text-ink-2">Found a bug, or want something in the next release?</p>
          <div className="flex flex-wrap gap-3">
            <a href={`${REPO_URL}/issues`} className={cn(keyVariants(), "no-underline")}>
              Open an issue
            </a>
            <a href={`${REPO_URL}/blob/main/CONTRIBUTING.md`} className={cn(keyVariants(), "no-underline")}>
              Contributing
            </a>
          </div>
        </Plate>
      </main>
      <SiteFooter />
    </div>
  );
}
