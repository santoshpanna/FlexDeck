import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { Glyph, cn } from "@flexdeck/ui";

import { Icon } from "../../../../common/components/Icon/Icon.component.tsx";
import { searchIcons } from "../../../../common/icons/icons.lib.ts";
import { fileAsBase64, type IconPack, type IconPackSummary } from "../../../../services/api/api.ts";
import { control } from "../Flow/fields.tsx";
import { uploadPicture, useDesk } from "../PicturePicker/PicturePicker.component.tsx";

/** At most this many icons are drawn at once; searching narrows the rest. */
const SHOWN = 140;

/**
 * Choose a key's icon: the bundled set, any installed icon pack, or a picture
 * of your own. Packs are added here too — a zip of PNGs or SVGs, or a pack in
 * the common manifest.json + icons.json layout other deck apps use.
 */
export function IconPicker({ value, onPick }: { value: string | undefined; onPick: (icon: string | null) => void }) {
  const { api, desktopId } = useDesk();
  const [source, setSource] = useState("bundled");
  const [query, setQuery] = useState("");
  const [packs, setPacks] = useState<IconPackSummary[]>([]);
  const [pack, setPack] = useState<IconPack | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const zip = useRef<HTMLInputElement>(null);
  const picture = useRef<HTMLInputElement>(null);

  const loadPacks = () => {
    if (!api || !desktopId) return;
    void api
      .iconPacks(desktopId)
      .then((answer) => setPacks(answer.packs ?? []))
      .catch(() => {});
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(loadPacks, [api, desktopId]);
  useEffect(() => {
    setPack(null);
    if (source === "bundled" || !api || !desktopId) return;
    let live = true;
    void api.iconPack(desktopId, source).then((found) => live && setPack(found));
    return () => {
      live = false;
    };
  }, [api, desktopId, source]);

  const bundled = useMemo(() => searchIcons(query), [query]);
  const fromPack = useMemo(() => {
    const words = query.trim().toLowerCase();
    return (pack?.icons ?? []).filter((icon) => !words || icon.name.toLowerCase().includes(words) || icon.tags?.some((tag) => tag.toLowerCase().includes(words)));
  }, [pack, query]);

  const tab = (id: string, label: string) => (
    <button
      key={id}
      type="button"
      aria-pressed={source === id}
      onClick={() => setSource(id)}
      className={cn("shrink-0 rounded-md px-2 py-1 text-[11px]", source === id ? "bg-signal-soft text-signal-hi" : "text-muted hover:text-ink")}
    >
      {label}
    </button>
  );

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-1 overflow-x-auto">
        {tab("bundled", "Built-in")}
        {packs.map((p) => tab(p.id, `${p.name} · ${p.count}`))}
      </div>
      <input aria-label="Search icons" placeholder="Search icons" value={query} onChange={(event) => setQuery(event.target.value)} className={control} />
      <div role="group" aria-label="Icons" className="grid max-h-44 grid-cols-7 gap-1.5 overflow-y-auto rounded-xl border border-line bg-well p-2 shadow-well">
        <Choice on={!value} title="No icon" onClick={() => onPick(null)}>
          <span className="text-faint">—</span>
        </Choice>
        {source === "bundled"
          ? bundled.map((icon) => (
              <Choice key={icon.name} on={value === icon.name} title={icon.title} onClick={() => onPick(icon.name)}>
                <Icon name={icon.name} size={18} decorative />
              </Choice>
            ))
          : fromPack.slice(0, SHOWN).map((icon) => {
              const name = `pack:${source}/${icon.name}`;
              return (
                <Choice key={icon.name} on={value === name} title={icon.name} onClick={() => onPick(name)}>
                  <Icon name={name} size={18} decorative />
                </Choice>
              );
            })}
        {source !== "bundled" && fromPack.length > SHOWN ? <p className="col-span-7 m-0 text-center text-[11px] text-faint">{fromPack.length - SHOWN} more — search to find them</p> : null}
      </div>
      {api && desktopId ? (
        <div className="flex flex-wrap items-center gap-3">
          <button type="button" disabled={busy !== null} onClick={() => picture.current?.click()} className="flex items-center gap-1 text-[12px] text-signal hover:underline disabled:opacity-50">
            <Glyph name="plus" size={11} /> {busy === "picture" ? "Uploading…" : "Your own picture"}
          </button>
          <button type="button" disabled={busy !== null} onClick={() => zip.current?.click()} className="flex items-center gap-1 text-[12px] text-signal hover:underline disabled:opacity-50">
            <Glyph name="plus" size={11} /> {busy === "pack" ? "Adding…" : "Add an icon pack"}
          </button>
          {source !== "bundled" ? (
            <button
              type="button"
              onClick={() => {
                void api.removeIconPack(desktopId, source).then(() => {
                  setSource("bundled");
                  loadPacks();
                });
              }}
              className="ml-auto text-[11px] text-muted hover:text-bad"
            >
              Remove this pack
            </button>
          ) : null}
          <input
            ref={picture}
            type="file"
            accept="image/png,image/jpeg,image/webp"
            className="hidden"
            onChange={(event) => {
              const file = event.target.files?.[0];
              event.target.value = "";
              if (!file) return;
              setBusy("picture");
              uploadPicture(api, desktopId, file)
                .then((image) => onPick(image))
                .catch((problem: unknown) => setNotice(String(problem instanceof Error ? problem.message : problem)))
                .finally(() => setBusy(null));
            }}
          />
          <input
            ref={zip}
            type="file"
            accept=".zip,application/zip"
            className="hidden"
            onChange={(event) => {
              const file = event.target.files?.[0];
              event.target.value = "";
              if (!file) return;
              setBusy("pack");
              setNotice(null);
              fileAsBase64(file)
                .then((data) => api.installIconPack(desktopId, data))
                .then((installed) => {
                  loadPacks();
                  setSource(installed.pack.id);
                  setNotice(
                    `Added ${installed.pack.count} icons from ${installed.pack.name}` +
                      (installed.skipped.length ? ` — ${installed.skipped.length} could not be used (${installed.skipped[0]})` : "."),
                  );
                })
                .catch((problem: unknown) => setNotice(String(problem instanceof Error ? problem.message : problem)))
                .finally(() => setBusy(null));
            }}
          />
        </div>
      ) : null}
      {notice ? <p className="m-0 text-[11px] text-muted">{notice}</p> : null}
    </div>
  );
}

function Choice({ on, title, onClick, children }: { on: boolean; title: string; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-pressed={on}
      title={title}
      aria-label={title}
      onClick={onClick}
      className={cn(
        "flex aspect-square cursor-pointer items-center justify-center rounded-lg border transition-colors",
        on ? "border-signal bg-signal-soft text-signal-hi" : "border-transparent text-ink-2 hover:border-line hover:bg-plate",
      )}
    >
      {children}
    </button>
  );
}
