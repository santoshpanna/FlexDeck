import { useRef, useState } from "react";

import { Glyph, Key, Label, Notice, cn } from "@flexdeck/ui";

import { fileAsBase64, type DataSource, type WireConfig, type WirePage } from "../../../../services/api/api.ts";
import { exportPage, importBundle, importedSources, parseBundle, picturesOf, reviewBundle, type Bundle, type Review } from "../../lib/share.lib.ts";
import { useDesk } from "../PicturePicker/PicturePicker.component.tsx";

type PageEdit = (transform: (config: WireConfig, pageId: string) => WireConfig) => void;

/**
 * Share this page as a file, or add one someone shared. Secrets never go:
 * the deck does not hold them. What an incoming page would run, type or
 * reach is listed before anything is added.
 */
export function ShareCard({ page, schema, onEdit }: { page: WirePage | null | undefined; schema?: number; onEdit: PageEdit }) {
  const { api, desktopId } = useDesk();
  const file = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [incoming, setIncoming] = useState<{ bundle: Bundle; review: Review; yours: DataSource[] } | null>(null);

  if (!api || !desktopId) return null;

  const fail = (problem: unknown) => setNotice(String(problem instanceof Error ? problem.message : problem));

  const share = async () => {
    setBusy("export");
    setNotice(null);
    try {
      const yours = (await api.sources(desktopId)).sources ?? [];
      const bundle = exportPage(page, schema, yours);
      if (!bundle) return;
      const images: Record<string, string> = {};
      let lost = 0;
      for (const sha of picturesOf(bundle.page)) {
        const url = await api.imageUrl(desktopId, sha);
        if (!url) {
          lost += 1;
          continue;
        }
        images[sha] = await fileAsBase64(await (await fetch(url)).blob());
      }
      if (Object.keys(images).length > 0) bundle.images = images;
      const name = (page?.name || "page").replace(/[^\w -]+/g, "").trim() || "page";
      const link = document.createElement("a");
      link.href = URL.createObjectURL(new Blob([JSON.stringify(bundle, null, 2)], { type: "application/json" }));
      link.download = `${name}.fddeck`;
      link.click();
      setTimeout(() => URL.revokeObjectURL(link.href), 10_000);
      setNotice(lost > 0 ? `Saved — without ${lost} picture${lost === 1 ? "" : "s"}, which can only be read on this computer.` : "Saved. Tokens and keys are never in it.");
    } catch (problem) {
      fail(problem);
    } finally {
      setBusy(null);
    }
  };

  const open = async (chosen: File) => {
    setBusy("read");
    setNotice(null);
    try {
      const bundle = parseBundle(await chosen.text());
      const [data, catalogue] = await Promise.all([api.sources(desktopId), api.pluginCatalogue(desktopId).catch(() => ({ plugins: [] }))]);
      const yours = data.sources ?? [];
      setIncoming({ bundle, yours, review: reviewBundle(bundle, catalogue.plugins.map((plugin) => plugin.id), yours) });
    } catch (problem) {
      fail(problem);
    } finally {
      setBusy(null);
    }
  };

  const add = async () => {
    if (!incoming) return;
    setBusy("import");
    try {
      const pictures: Record<string, string> = {};
      for (const [sha, data] of Object.entries(incoming.bundle.images ?? {})) {
        pictures[sha] = (await api.uploadPicture(desktopId, data)).image;
      }
      // The sources first: the page reads them, renamed or not, the same way.
      if ((incoming.bundle.sources ?? []).length > 0) await api.saveSources(desktopId, importedSources(incoming.bundle, incoming.yours).sources);
      onEdit((config, pageId) => {
        const profile = (config.profiles ?? []).find((p) => (p.pages ?? []).some((candidate) => candidate.id === pageId));
        return profile ? importBundle(config, profile.id, incoming.bundle, incoming.yours, pictures).config : config;
      });
      setNotice(`Added “${incoming.review.name}” as a new page.`);
      setIncoming(null);
    } catch (problem) {
      fail(problem);
    } finally {
      setBusy(null);
    }
  };

  const review = incoming?.review;

  return (
    <section className="flex flex-col gap-2.5">
      <Label>Share</Label>
      <div className="flex flex-wrap gap-2">
        <Key size="sm" disabled={!page || busy !== null} onClick={() => void share()}>
          <Glyph name="file" size={11} /> {busy === "export" ? "Saving…" : "Save this page as a file"}
        </Key>
        <Key size="sm" tone="ghost" disabled={busy !== null} onClick={() => file.current?.click()}>
          <Glyph name="plus" size={11} /> {busy === "read" ? "Reading…" : "Add a shared page"}
        </Key>
        <input
          ref={file}
          type="file"
          accept=".fddeck,application/json"
          className="hidden"
          onChange={(event) => {
            const chosen = event.target.files?.[0];
            event.target.value = "";
            if (chosen) void open(chosen);
          }}
        />
      </div>

      {review ? (
        <div className="flex flex-col gap-2.5 rounded-xl border border-line-strong bg-plate p-3">
          <p className="m-0 text-[13px] text-ink">
            “{review.name}”: {review.keys} key{review.keys === 1 ? "" : "s"}
            {review.pictures ? `, ${review.pictures} picture${review.pictures === 1 ? "" : "s"}` : ""}
            {review.sources.length ? `, ${review.sources.length} data source${review.sources.length === 1 ? "" : "s"}` : ""}.
          </p>
          {review.concerns.length > 0 ? (
            <div className="flex flex-col gap-1">
              <span className="lbl">Before you add it — it would</span>
              <ul className="m-0 flex list-none flex-col gap-1 p-0">
                {review.concerns.map((concern, index) => (
                  <li key={index} className={cn("text-[12px]", concern.severe ? "text-bad" : "text-ink-2")}>
                    {concern.severe ? "⚠ " : "· "}
                    <span className="text-muted">{concern.where}</span> {concern.what}
                  </li>
                ))}
              </ul>
            </div>
          ) : (
            <p className="m-0 text-[12px] text-muted">It runs no programs, types nothing and reaches nothing outside.</p>
          )}
          {review.missing.length > 0 ? (
            <Notice tone="warn">Needs plugins this desktop does not have: {review.missing.join(", ")}. Its keys that use them will do nothing until they are installed.</Notice>
          ) : null}
          {review.sources.some((s) => s.renamed) ? (
            <p className="m-0 text-[11px] text-faint">
              Renamed to keep yours: {review.sources.filter((s) => s.renamed).map((s) => `${s.id} → ${s.renamed}`).join(", ")}.
            </p>
          ) : null}
          {review.sources.length > 0 ? <p className="m-0 text-[11px] text-faint">Tokens and keys its sources need are not in it: set them under Data.</p> : null}
          <div className="flex gap-2">
            <Key size="sm" tone="signal" disabled={busy !== null} onClick={() => void add()}>
              {busy === "import" ? "Adding…" : "Add as a new page"}
            </Key>
            <Key size="sm" tone="ghost" onClick={() => setIncoming(null)}>
              Cancel
            </Key>
          </div>
        </div>
      ) : null}

      {notice ? <p className="m-0 text-[11px] text-muted">{notice}</p> : null}
    </section>
  );
}
