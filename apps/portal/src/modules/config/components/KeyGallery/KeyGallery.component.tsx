import { useMemo, useState } from "react";

import { cn } from "@flexdeck/ui";

import type { WireConfig, WireWidget } from "../../../../services/api/api.ts";
import { useFacePreviews } from "../../hooks/useFacePreviews/useFacePreviews.hook.ts";
import { usePresets } from "../../hooks/usePresets/usePresets.hook.ts";
import { resizeWidget } from "../../lib/editing.lib.ts";
import { applyPresetLook, applyWholePreset, isLookPreset, presetFits, type Preset, type PresetCategory } from "../../lib/presets.lib.ts";
import { PresetCard } from "../PresetCard/PresetCard.component.tsx";

type WidgetEdit = (transform: (config: WireConfig, widgetId: string) => WireConfig) => void;

type Applied = { preset: Preset; how: "whole" | "look" | "replaced"; grow: boolean; extra?: string };

/**
 * Ready-made keys, for this key. One that does nothing yet becomes the
 * preset, look and press. One that already does something keeps that and
 * takes just the look — unless asked to take the press too.
 */
export function KeyGallery({ widget, onEdit }: { widget: WireWidget; onEdit: WidgetEdit }) {
  const { presets, categories, ensureSources, desktopId } = usePresets();
  const [category, setCategory] = useState<PresetCategory>("System");
  const shown = presets.filter((preset) => preset.category === category);
  const widgets = useMemo<WireWidget[]>(
    () => shown.map((preset) => ({ ...structuredClone(preset.widget), id: `gallery-${preset.id}` })),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [category, presets.length],
  );
  const previews = useFacePreviews(desktopId, widgets);
  const [applied, setApplied] = useState<Applied | null>(null);
  const does = (widget.flows?.length ?? 0) > 0;

  const apply = (preset: Preset, whole: boolean) => {
    let grow = false;
    const how: Applied["how"] = whole || !does ? "whole" : isLookPreset(preset) ? "look" : "replaced";
    onEdit((config, id) => {
      if (how !== "look") return applyWholePreset(config, id, preset);
      const next = applyPresetLook(config, id, preset);
      grow = presetFits(next, id, preset);
      return next;
    });
    setApplied({ preset, how, grow });
    void ensureSources([preset.needs])
      .then((extra) => extra && setApplied((was) => (was?.preset === preset ? { ...was, extra } : was)))
      .catch(() => {});
  };

  const link = "cursor-pointer text-muted underline-offset-2 hover:text-ink hover:underline";
  return (
    <div className="flex flex-col gap-3">
      <p className="m-0 text-[12px] leading-snug text-muted">
        {does ? "Pick one to give this key its look. What it does stays." : "Pick one and this key becomes it: its look and what it does."}
      </p>
      <div className="flex flex-wrap gap-1">
        {categories.map((name) => (
          <button
            key={name}
            type="button"
            aria-pressed={category === name}
            onClick={() => setCategory(name)}
            className={cn("cursor-pointer rounded-md px-2 py-1 text-[11px]", category === name ? "bg-plate-2 text-ink" : "text-muted hover:text-ink")}
          >
            {name}
          </button>
        ))}
      </div>
      <div className="grid grid-flow-dense grid-cols-2 gap-2">
        {shown.map((preset, index) => {
          const card = widgets[index]!;
          const chosen = applied?.preset.id === preset.id;
          const followUps = chosen ? (
            <>
              {applied.how === "look" && preset.widget.flows?.length ? (
                <button type="button" className={link} onClick={() => apply(preset, true)}>
                  Use its press too
                </button>
              ) : null}
              {applied.grow ? (
                <button
                  type="button"
                  className={link}
                  onClick={() => {
                    onEdit((config, id) => resizeWidget(config, id, preset.w, preset.h));
                    setApplied({ ...applied, grow: false });
                  }}
                >
                  Make it {preset.w}×{preset.h}
                </button>
              ) : null}
            </>
          ) : null;
          return <PresetCard key={preset.id} preset={preset} widget={card} preview={previews[card.id]} chosen={chosen} after={followUps} onClick={() => apply(preset, false)} />;
        })}
      </div>
    </div>
  );
}
