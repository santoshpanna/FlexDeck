import { useMemo, useRef, useState } from "react";

import { cn, Glyph, HoldToConfirm, Key, Label, Notice, Segmented } from "@flexdeck/ui";

import type { ActionDefinition, WireConfig } from "../../../../services/api/api.ts";
import { appendDeck, checkDeck, deckConcerns, exportDeck, pageCount, parseDeck, type ImportMode } from "../../lib/deck.file.lib.ts";

const MODES = [
  { value: "replace", label: "Replace" },
  { value: "append", label: "Append" },
] as const;

export function DeckFileCard({
  config,
  profileId,
  catalogue,
  onReplace,
}: {
  config: WireConfig | null;
  profileId: string | null;
  catalogue: readonly ActionDefinition[];
  onReplace: (next: WireConfig) => void;
}) {
  const file = useRef<HTMLInputElement>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [incoming, setIncoming] = useState<{ name: string; deck: WireConfig } | null>(null);
  const [mode, setMode] = useState<ImportMode>("replace");

  const save = () => {
    if (!config) return;
    const link = document.createElement("a");
    link.href = URL.createObjectURL(new Blob([exportDeck(config)], { type: "application/json" }));
    link.download = "flexdeck-deck.json";
    link.click();
    setTimeout(() => URL.revokeObjectURL(link.href), 10_000);
    setNotice("Exported.");
  };

  const open = async (chosen: File) => {
    setNotice(null);
    if (!config) return;
    try {
      setIncoming({ name: chosen.name, deck: parseDeck(await chosen.text(), config) });
    } catch (problem) {
      setNotice(problem instanceof Error ? problem.message : String(problem));
    }
  };

  const check = incoming && config ? checkDeck(incoming.deck, config, mode, profileId, catalogue) : null;
  const ready = check !== null && check.missing.length === 0 && check.problems.length === 0;
  const pages = incoming ? pageCount(incoming.deck) : 0;
  const concerns = useMemo(() => (incoming ? deckConcerns(incoming.deck) : []), [incoming]);

  const apply = () => {
    if (!incoming || !config || !ready) return;
    try {
      onReplace(mode === "replace" ? incoming.deck : appendDeck(config, profileId!, incoming.deck));
      setIncoming(null);
      setNotice(mode === "replace" ? "Imported. Undo brings back the deck you had." : `Added ${pages} page${pages === 1 ? "" : "s"}. Undo takes them away.`);
    } catch (problem) {
      setNotice(problem instanceof Error ? problem.message : String(problem));
    }
  };

  return (
    <section className="flex flex-col gap-2.5 rounded-plate border border-line bg-plate p-5">
      <Label>Deck file</Label>
      <div className="flex flex-wrap gap-2">
        <Key size="sm" disabled={!config} onClick={save}>
          <Glyph name="file" size={11} /> Export
        </Key>
        <Key size="sm" tone="ghost" disabled={!config} onClick={() => file.current?.click()}>
          <Glyph name="plus" size={11} /> Import
        </Key>
        <input
          ref={file}
          type="file"
          accept=".json,application/json"
          className="hidden"
          onChange={(event) => {
            const chosen = event.target.files?.[0];
            event.target.value = "";
            if (chosen) void open(chosen);
          }}
        />
      </div>

      {incoming && check ? (
        <div className="flex flex-col gap-2.5 rounded-xl border border-line-strong bg-plate p-3">
          <p className="m-0 text-[13px] text-ink">
            “{incoming.name}”: {pages} page{pages === 1 ? "" : "s"}.
          </p>
          <Segmented label="How to import" size="sm" value={mode} onChange={setMode} options={MODES} />
          <p className="m-0 text-[12px] text-muted">
            {mode === "replace" ? "Your deck is swapped for this one." : "Its pages are added after yours, on this grid."}
          </p>
          {concerns.length > 0 ? (
            <div className="flex flex-col gap-1">
              <span className="lbl">Before you import it — it would</span>
              <ul className="m-0 flex list-none flex-col gap-1 p-0">
                {concerns.slice(0, 40).map((concern, index) => (
                  <li key={index} className={cn("text-[12px]", concern.severe ? "text-bad" : "text-ink-2")}>
                    {concern.severe ? "⚠ " : "· "}
                    <span className="text-muted">{concern.where}</span> {concern.what}
                  </li>
                ))}
                {concerns.length > 40 ? <li className="text-[12px] text-muted">…and {concerns.length - 40} more.</li> : null}
              </ul>
            </div>
          ) : (
            <p className="m-0 text-[12px] text-muted">It runs no programs, types nothing and reaches nothing outside.</p>
          )}
          {check.missing.length > 0 ? <Notice>Install these plugins first: {check.missing.join(", ")}.</Notice> : null}
          {check.problems.length > 0 ? (
            <div className="flex flex-col gap-1">
              <span className="lbl">It would break — nothing was imported</span>
              <ul className="m-0 flex list-none flex-col gap-1 p-0">
                {check.problems.slice(0, 20).map((problem, index) => (
                  <li key={index} className="text-[12px] text-bad">
                    · {problem}
                  </li>
                ))}
                {check.problems.length > 20 ? <li className="text-[12px] text-muted">…and {check.problems.length - 20} more.</li> : null}
              </ul>
            </div>
          ) : null}
          <div className="flex gap-2">
            {mode === "replace" ? (
              <HoldToConfirm size="sm" tone="danger" disabled={!ready} onConfirm={apply}>
                Hold to replace
              </HoldToConfirm>
            ) : (
              <Key size="sm" tone="signal" disabled={!ready} onClick={apply}>
                Add {pages} page{pages === 1 ? "" : "s"}
              </Key>
            )}
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
