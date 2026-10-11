import { useContext, useEffect, useRef, useState } from "react";

import { Glyph, cn } from "@flexdeck/ui";

import { usePictureUrl } from "../../../../hooks/usePictureUrl/usePictureUrl.hook.ts";
import { ApiContext } from "../../../../providers/ApiProvider/ApiProvider.provider.tsx";
import { DesktopContext } from "../../../../providers/DesktopProvider/DesktopProvider.provider.tsx";
import { fileAsBase64 } from "../../../../services/api/api.ts";

/** The selected desktop and its API, read without the throwing hooks. */
export function useDesk() {
  const api = useContext(ApiContext);
  const desktopId = useContext(DesktopContext)?.selected?.id ?? null;
  return { api, desktopId };
}

/** Upload a file the person chose; answers its `img:` name, or throws why not. */
export async function uploadPicture(api: NonNullable<ReturnType<typeof useDesk>["api"]>, desktopId: string, file: File): Promise<string> {
  return (await api.uploadPicture(desktopId, await fileAsBase64(file))).image;
}

function Thumb({ image, size = 44 }: { image: string; size?: number }) {
  const url = usePictureUrl(image);
  return url ? (
    <img src={url} alt="" width={size} height={size} className="rounded-md object-cover" style={{ width: size, height: size }} />
  ) : (
    <span className="block rounded-md bg-well" style={{ width: size, height: size }} />
  );
}

/**
 * A picture for a background: upload one, or pick one already on the desktop.
 * Pictures are checked and made small by the desktop before they are kept.
 */
export function PicturePicker({ value, onPick }: { value: string | undefined; onPick: (image: string) => void }) {
  const { api, desktopId } = useDesk();
  const input = useRef<HTMLInputElement>(null);
  const [library, setLibrary] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (library === null || !api || !desktopId) return;
    let live = true;
    void api
      .pictures(desktopId)
      .then((answer) => live && setLibrary((answer.pictures ?? []).map((p) => `img:${p.sha256}`)))
      .catch(() => {});
    return () => {
      live = false;
    };
    // Loaded when opened; `library !== null` is the opened flag.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, desktopId, library === null]);

  if (!api || !desktopId) return null;
  const chosen = value?.startsWith("img:") ? value : null;

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        {chosen ? <Thumb image={chosen} size={32} /> : null}
        <button
          type="button"
          disabled={busy}
          onClick={() => input.current?.click()}
          className="flex h-8 cursor-pointer items-center gap-1.5 rounded-lg border border-line bg-plate px-3 text-[12px] hover:border-line-strong disabled:opacity-50"
        >
          <Glyph name="plus" size={11} /> {busy ? "Uploading…" : "Upload a picture"}
        </button>
        <button type="button" onClick={() => setLibrary(library === null ? [] : null)} className="text-[12px] text-signal hover:underline">
          {library === null ? "Or pick one" : "Close"}
        </button>
        {chosen ? (
          <button type="button" onClick={() => onPick("")} className="ml-auto text-[12px] text-muted hover:text-ink">
            Remove
          </button>
        ) : null}
        <input
          ref={input}
          type="file"
          accept="image/png,image/jpeg,image/webp"
          className="hidden"
          onChange={(event) => {
            const file = event.target.files?.[0];
            event.target.value = "";
            if (!file) return;
            setBusy(true);
            setError(null);
            uploadPicture(api, desktopId, file)
              .then((image) => {
                onPick(image);
                setLibrary((list) => (list === null ? null : [image, ...list.filter((i) => i !== image)]));
              })
              .catch((problem: unknown) => setError(String(problem instanceof Error ? problem.message : problem)))
              .finally(() => setBusy(false));
          }}
        />
      </div>
      {error ? <p className="m-0 text-[11px] text-bad">{error}</p> : null}
      {library !== null ? (
        <div className="grid max-h-40 grid-cols-6 gap-1.5 overflow-y-auto rounded-lg border border-line bg-well p-1.5 shadow-well">
          {library.length === 0 ? <p className="col-span-6 m-0 p-2 text-[11px] text-faint">No pictures on this desktop yet.</p> : null}
          {library.map((image) => (
            <button
              key={image}
              type="button"
              aria-pressed={image === chosen}
              onClick={() => onPick(image)}
              className={cn("cursor-pointer rounded-md border-2", image === chosen ? "border-signal" : "border-transparent hover:border-line-strong")}
            >
              <Thumb image={image} />
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
