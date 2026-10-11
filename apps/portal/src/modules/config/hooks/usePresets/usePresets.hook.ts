import { useEffect, useState } from "react";

import { useDesk } from "../../components/PicturePicker/PicturePicker.component.tsx";
import { PRESETS, PRESET_CATEGORIES, type Preset, type PresetCategory } from "../../lib/presets.lib.ts";
import { STARTERS } from "../../lib/sources.lib.ts";

/**
 * The ready-made keys: the built-in ones and those the desktop's plugins
 * bring (ABI 4, under "Plugins"), with what adding one needs done first.
 */
export function usePresets() {
  const { api, desktopId } = useDesk();
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

  /**
   * Add the data sources these presets read, when the desktop does not have
   * them yet. Answers a line saying what was added, or `null` for nothing.
   */
  const ensureSources = async (needs: readonly Preset["needs"][]): Promise<string | null> => {
    const wanted = needs.filter((n): n is NonNullable<Preset["needs"]> => n !== undefined);
    if (!api || !desktopId || wanted.length === 0) return null;
    const current = (await api.sources(desktopId)).sources ?? [];
    const missing = wanted.filter((need) => !current.some((source) => source.id === need.id));
    if (missing.length === 0) return null;
    const added = missing.flatMap((need) => {
      const starter = STARTERS.find((s) => s.label === need.starter);
      return starter ? [{ ...structuredClone(starter.source), id: need.id }] : [];
    });
    await api.saveSources(desktopId, [...current, ...added]);
    return `Added the ${added.map((s) => s.name).join(" and ")} data source${added.length > 1 ? "s" : ""} — change its settings under Data.`;
  };

  return { presets: [...PRESETS, ...fromPlugins], categories, ensureSources, desktopId };
}
