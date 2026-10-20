import { motion } from "motion/react";
import type { ReactNode } from "react";
import { Link } from "react-router-dom";

import { Plate, cn, ease, keyVariants } from "@flexdeck/ui";

import { SiteNav } from "../../../common/components/SiteNav/SiteNav.component.tsx";
import { SiteFooter } from "../../../common/components/SiteFooter/SiteFooter.component.tsx";
import { type Asset, type Platform, RELEASES_URL, REPO_URL } from "../../../common/release/release.lib.ts";
import { useDocumentTitle } from "../../../hooks/useDocumentTitle/useDocumentTitle.hook.ts";

export interface DownloadViewProps {
  version: string | null;
  date: string | null;
  platform: Platform;
  windows: Asset;
  android: Asset;
  checksums: Asset;
}

function DownloadIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <polyline points="7 10 12 15 17 10" />
      <line x1="12" y1="15" x2="12" y2="3" />
    </svg>
  );
}

function Spec({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-1.5">
      <dt className="lbl text-[9px]">{label}</dt>
      <dd className="m-0 font-mono text-xs text-ink">{value}</dd>
    </div>
  );
}

/** One platform: what it is, the button, and what it needs. The visitor's own platform gets the signal key. */
function PlatformCard({ tag, title, body, asset, label, here, specs, children }: { tag: string; title: string; body: string; asset: Asset; label: string; here: boolean; specs: [string, string][]; children?: ReactNode }) {
  return (
    <Plate pad="none" className="flex flex-col gap-5 p-7">
      <div className="flex items-start justify-between gap-3">
        <span className="font-mono text-[10px] tracking-[0.2em] text-signal uppercase">{tag}</span>
        {here ? (
          <span className="flex items-center gap-2 font-mono text-[10px] tracking-[0.2em] text-muted uppercase">
            <span aria-hidden="true" className="led-glow size-1.5 rounded-full bg-signal text-signal" />
            This device
          </span>
        ) : null}
      </div>
      <h2 className="m-0 text-[30px] leading-tight font-semibold tracking-[-0.02em]">{title}</h2>
      <p className="m-0 text-[15px] leading-relaxed text-muted">{body}</p>
      <div className="flex flex-wrap items-center gap-4">
        <a href={asset.url} className={cn(keyVariants({ tone: here ? "signal" : "default", size: "lg" }), "h-13 px-6 no-underline")}>
          <DownloadIcon />
          {label}
        </a>
        <span className="font-mono text-[11px] tracking-[0.12em] text-faint">{asset.file}</span>
      </div>
      {children}
      <dl className="m-0 grid grid-cols-3 gap-3 border-t border-line pt-5">
        {specs.map(([key, value]) => (
          <Spec key={key} label={key} value={value} />
        ))}
      </dl>
    </Plate>
  );
}

/**
 * The download page: both apps, every file, how to install and check them, and
 * how to build from source. Pure markup over the controller's props.
 */
export function DownloadView(props: Partial<DownloadViewProps>) {
  const { version = null, date = null, platform = "other", windows, android, checksums } = props;
  useDocumentTitle("Download");
  if (!windows || !android || !checksums) return null;
  const files = [windows, android, checksums];

  return (
    <div className="min-h-screen bg-body text-ink">
      <SiteNav />
      <main className="mx-auto flex max-w-[1200px] flex-col gap-16 px-[clamp(16px,4vw,48px)] pt-20 pb-32">
        <motion.header
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.8, ease: ease.outExpo }}
          className="flex flex-wrap items-end justify-between gap-12"
        >
          <div className="flex min-w-0 flex-[1_1_460px] flex-col gap-6">
            <span className="lbl text-signal">// download</span>
            <h1 className="m-0 font-mono text-[clamp(36px,5.4vw,82px)] leading-[0.98] font-medium tracking-[-0.03em]">
              Two installs.
              <br />
              Then it’s yours.
            </h1>
            <p className="m-0 max-w-[480px] text-lg leading-relaxed text-ink-2">
              The desktop app runs your actions and plugins. The tablet app is the deck. Pair them with six digits. No account, no cloud.
            </p>
          </div>
          <div className="flex flex-col gap-2 font-mono text-[11px] tracking-[0.18em] text-faint uppercase">
            {version ? (
              <>
                <span>
                  Latest · <span className="text-ink">{version}</span>
                </span>
                <span>{date}</span>
              </>
            ) : (
              <span>First release coming soon</span>
            )}
            <Link to="/changelog" className="text-signal no-underline hover:text-signal-hi">
              What’s new →
            </Link>
          </div>
        </motion.header>

        <div className={cn("grid gap-4 [grid-template-columns:repeat(auto-fit,minmax(min(100%,420px),1fr))]", platform === "android" && "[&>*:first-child]:order-2")}>
          <PlatformCard
            tag="01 · The PC"
            title="flexdeck for Windows"
            body="Sits in the tray, serves your tablets over Wi-Fi and runs plugins on this computer. Allow it on private networks when Windows asks."
            asset={windows}
            label="Download for Windows"
            here={platform !== "android"}
            specs={[
              ["Needs", "Windows 10 / 11"],
              ["Arch", "x64"],
              ["Runs in", "The tray"],
            ]}
          />
          <PlatformCard
            tag="02 · The tablet"
            title="flexdeck for Android"
            body="Native and light, so it runs on the 2015 tablet in your drawer. A Google Play listing is coming; until then, install the APK."
            asset={android}
            label="Get the Android app"
            here={platform === "android"}
            specs={[
              ["Needs", "Android 5.0+"],
              ["Works on", "Tablet, phone"],
              ["Play Store", "Coming"],
            ]}
          >
            {platform !== "android" ? (
              <Plate depth="well" pad="none" className="px-4 py-3 text-[13px] leading-relaxed text-muted">
                On this PC? Open <span className="font-mono text-ink">flex-deck.com/download</span> in the tablet’s browser to install it there.
              </Plate>
            ) : null}
          </PlatformCard>
        </div>

        <section aria-labelledby="files" className="flex flex-col gap-5">
          <div className="flex flex-wrap items-end justify-between gap-3">
            <div>
              <span className="lbl">§ Files</span>
              <h2 id="files" className="m-0 mt-2 text-[clamp(26px,2.6vw,36px)] font-semibold tracking-[-0.02em]">
                {version ? `Everything in ${version}` : "Every file"}
              </h2>
            </div>
            <a href={RELEASES_URL} className="font-mono text-[11px] tracking-[0.18em] text-signal uppercase no-underline hover:text-signal-hi">
              Older releases →
            </a>
          </div>
          <Plate pad="none" className="overflow-x-auto">
            <table className="w-full min-w-[620px] border-collapse text-sm">
              <thead>
                <tr>
                  <th className="lbl px-5 py-4 text-left font-normal">Platform</th>
                  <th className="lbl px-5 py-4 text-left font-normal">File</th>
                  <th className="px-5 py-4">
                    <span className="sr-only">Download</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {files.map((file) => (
                  <tr key={file.file} className="border-t border-line">
                    <td className="px-5 py-4 font-semibold">{file.platform}</td>
                    <td className="px-5 py-4 font-mono text-ink-2">{file.file}</td>
                    <td className="px-5 py-4 text-right">
                      <a href={file.url} className={cn(keyVariants({ size: "sm" }), "no-underline")}>
                        Download
                      </a>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </Plate>
        </section>

        <ol className="m-0 grid list-none gap-8 p-0 [grid-template-columns:repeat(auto-fit,minmax(min(100%,300px),1fr))]">
          <li className="flex flex-col gap-3.5 border-t border-line pt-6">
            <span className="font-mono text-[10px] tracking-[0.2em] text-signal uppercase">01 · PC</span>
            <h3 className="m-0 text-[22px] font-semibold">Run the installer</h3>
            <p className="m-0 leading-relaxed text-muted">flexdeck starts in the tray. Closing the window only hides it; quit from the tray.</p>
          </li>
          <li className="flex flex-col gap-3.5 border-t border-line pt-6">
            <span className="font-mono text-[10px] tracking-[0.2em] text-signal uppercase">02 · Tablet</span>
            <h3 className="m-0 text-[22px] font-semibold">Install the APK</h3>
            <p className="m-0 leading-relaxed text-muted">Allow installs from your browser when Android asks, then type the PC’s address and six-digit code.</p>
          </li>
          <li className="flex flex-col gap-3.5 border-t border-line pt-6">
            <span className="font-mono text-[10px] tracking-[0.2em] text-signal uppercase">03 · Verify</span>
            <h3 className="m-0 text-[22px] font-semibold">Check the hash</h3>
            <p className="m-0 leading-relaxed text-muted">
              Compare it with <a href={checksums.url} className="text-signal">{checksums.file}</a>.
            </p>
            <Plate depth="well" pad="none" className="overflow-x-auto px-4 py-3">
              <code className="font-mono text-xs whitespace-pre text-ink-2">
                <span className="text-faint">PS&gt; </span>Get-FileHash .\{windows.file}
              </code>
            </Plate>
          </li>
        </ol>

        <Plate pad="none" className="flex flex-wrap items-center justify-between gap-7 p-7">
          <div className="flex min-w-0 flex-[1_1_340px] flex-col gap-2">
            <h2 className="m-0 text-[22px] font-semibold">Build it yourself</h2>
            <p className="m-0 text-[15px] leading-relaxed text-muted">
              AGPL-3.0, with MIT plugin SDKs. Needs Node 22+, Rust 1.88+ and JDK 17 for Android.{" "}
              <Link to="/docs/development" className="text-signal">
                Development guide →
              </Link>
            </p>
          </div>
          <Plate depth="well" pad="none" className="min-w-0 flex-[1_1_420px] overflow-x-auto px-4 py-3.5">
            <code className="font-mono text-xs leading-[1.7] whitespace-pre text-ink-2">
              {`git clone ${REPO_URL}.git\npnpm install\npnpm dev:desktop`}
            </code>
          </Plate>
        </Plate>
      </main>
      <SiteFooter />
    </div>
  );
}
