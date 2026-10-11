import { useEffect, useMemo, useState } from "react";

import { Glyph, Label, Notice, cn } from "@flexdeck/ui";

import type { WireConfig, WireWidget } from "../../../../services/api/api.ts";
import { useFacePreviews } from "../../hooks/useFacePreviews/useFacePreviews.hook.ts";
import { PRESETS, PRESET_CATEGORIES, STARTER_LAYOUTS, placeLayout, placePreset, type Preset, type PresetCategory } from "../../lib/presets.lib.ts";
import { STARTERS } from "../../lib/sources.lib.ts";
import { FaceView } from "../FaceView/FaceView.component.tsx";
import { useDesk } from "../PicturePicker/PicturePicker.component.tsx";

type PageEdit = (transform: (config: WireConfig, pageId: string) => WireConfig) => void;

/** A card's key is drawn this many pixels a cell, at the tablet's unit. */
const CELL = 92;

/**
 * Ready-made keys, drawn live: drop one on the page and it works. A key that
 * reads a data source brings the source with it.
 */
export function Gallery({ onEdit, empty }: { onEdit: PageEdit; empty: boolean }) {
  const { api, desktopId } = useDesk();
  const [category, setCategory] = useState<PresetCategory>("System");
  const [notice, setNotice] = useState<string | null>(null);
  // Plugins' own presets (ABI 4), under "Plugins".
  const [fromPlugins, setFromPlugins] = useState<Preset[]>([]);
  useEffect(() => {
    if (!api || !desktopId) return;
    let live = true;
    void api
      .pluginCatalogue(desktopId)
      .then((answer) => {
        if (!live) return;
        setFromPlugins(
          answer.plugins.flatMap((plugin) =>
            (plugin.presets ?? []).map(
              (preset): Preset => ({
                id: `${plugin.id}/${preset.id}`,
                name: preset.name,
                category: "Plugins",
                description: preset.description || plugin.name,
                w: preset.w ?? 1,
                h: preset.h ?? 1,
                widget: preset.widget as Preset["widget"],
              }),
            ),
          ),
        );
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [api, desktopId]);
  const categories: PresetCategory[] = fromPlugins.length > 0 ? [...PRESET_CATEGORIES, "Plugins"] : [...PRESET_CATEGORIES];
  const shown = [...PRESETS, ...fromPlugins].filter((preset) => preset.category === category);
  const widgets = useMemo<WireWidget[]>(
    () => shown.map((preset) => ({ ...structuredClone(preset.widget), id: `gallery-${preset.id}` })),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [category, fromPlugins],
  );
  const previews = useFacePreviews(desktopId, widgets);

  /** Add the sources these presets read, when the desktop does not have them yet. */
  const ensureSources = async (needs: Preset["needs"][]) => {
    const wanted = needs.filter((n): n is NonNullable<Preset["needs"]> => n !== undefined);
    if (!api || !desktopId || wanted.length === 0) return;
    const current = (await api.sources(desktopId)).sources ?? [];
    const missing = wanted.filter((need) => !current.some((source) => source.id === need.id));
    if (missing.length === 0) return;
    const added = missing.flatMap((need) => {
      const starter = STARTERS.find((s) => s.label === need.starter);
      return starter ? [{ ...structuredClone(starter.source), id: need.id }] : [];
    });
    await api.saveSources(desktopId, [...current, ...added]);
    setNotice(`Added the ${added.map((s) => s.name).join(" and ")} data source${added.length > 1 ? "s" : ""} — change its settings under Data.`);
  };

  const add = (preset: Preset) => {
    setNotice(null);
    let placed = false;
    onEdit((config, pageId) => {
      const result = placePreset(config, pageId, preset);
      placed = result !== null;
      return result?.config ?? config;
    });
    if (!placed) {
      setNotice("There is no room left on this page. Make some, or add a page.");
      return;
    }
    void ensureSources([preset.needs]).catch((problem: unknown) => setNotice(String(problem instanceof Error ? problem.message : problem)));
  };

  const layout = (id: string) => {
    setNotice(null);
    let needs: Preset["needs"][] = [];
    onEdit((config, pageId) => {
      const result = placeLayout(config, pageId, id);
      needs = result.needs;
      return result.config;
    });
    void ensureSources(needs).catch(() => {});
  };

  return (
    <section className="flex flex-col gap-3">
      {empty ? (
        <div className="flex flex-col gap-2">
          <Label>Start this page from</Label>
          {STARTER_LAYOUTS.map((starter) => (
            <button
              key={starter.id}
              type="button"
              onClick={() => layout(starter.id)}
              className="flex flex-col items-start gap-0.5 rounded-xl border border-line bg-plate px-3 py-2.5 text-left hover:border-signal"
            >
              <span className="text-[13px] font-semibold">{starter.name}</span>
              <span className="text-[11px] text-muted">{starter.description}</span>
            </button>
          ))}
        </div>
      ) : null}

      <div className="flex items-baseline justify-between">
        <Label>Gallery</Label>
        <span className="text-[11px] text-faint">Click one to add it</span>
      </div>
      <div className="flex flex-wrap gap-1">
        {categories.map((name) => (
          <button
            key={name}
            type="button"
            aria-pressed={category === name}
            onClick={() => setCategory(name)}
            className={cn("rounded-md px-2 py-1 text-[11px]", category === name ? "bg-signal-soft text-signal-hi" : "text-muted hover:text-ink")}
          >
            {name}
          </button>
        ))}
      </div>
      {notice ? <Notice tone="ok" onDismiss={() => setNotice(null)}>{notice}</Notice> : null}
      <div className="grid grid-cols-2 gap-2">
        {shown.map((preset, index) => {
          const widget = widgets[index]!;
          const w = preset.w * CELL;
          const h = preset.h * CELL;
          const scale = Math.min(1, 120 / w, 92 / h);
          return (
            <button
              key={preset.id}
              type="button"
              title={preset.description}
              onClick={() => add(preset)}
              className={cn(
                "group flex flex-col items-center gap-1.5 rounded-xl border border-line bg-plate p-2 text-left transition-colors hover:border-signal",
                preset.w > 1 && "col-span-2",
              )}
            >
              <span className="relative flex items-center justify-center" style={{ width: w * scale, height: h * scale }}>
                <span
                  className="absolute top-0 left-0 overflow-hidden rounded-[14px] border border-[var(--fd-deck-key-line)] bg-linear-to-b from-[var(--fd-deck-key-top)] to-[var(--fd-deck-key-bottom)] text-[var(--fd-deck-ink)]"
                  style={{ width: w, height: h, transform: `scale(${scale})`, transformOrigin: "top left" }}
                >
                  <FaceView widget={widget} w={w} h={h} unit={0.72} radius={14} preview={previews[widget.id]} />
                </span>
              </span>
              <span className="flex w-full items-center justify-between gap-2">
                <span className="truncate text-[12px] font-semibold">{preset.name}</span>
                <Glyph name="plus" size={12} />
              </span>
              <span className="line-clamp-2 w-full text-[11px] leading-snug text-muted">{preset.description}</span>
            </button>
          );
        })}
      </div>
    </section>
  );
}
