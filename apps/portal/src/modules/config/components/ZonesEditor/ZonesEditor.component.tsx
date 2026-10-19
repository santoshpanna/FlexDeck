import type { Zone } from "@flexdeck/face";
import { useState } from "react";

import { Glyph, Label, Segmented, cn } from "@flexdeck/ui";

import { Icon } from "../../../../common/components/Icon/Icon.component.tsx";
import type { VariableChoice, WireConfig, WireWidget } from "../../../../services/api/api.ts";
import { MUSIC_ZONES, editFace, renameZone, setZones, zoneName } from "../../lib/face.edit.lib.ts";
import { DraftInput, ExpressionInput, control } from "../Flow/fields.tsx";
import { IconPicker } from "../IconPicker/IconPicker.component.tsx";

type WidgetEdit = (transform: (config: WireConfig, widgetId: string) => WireConfig) => void;

/**
 * Buttons along the bottom of a key — a music key's previous, play and next —
 * each with its own action (set under Does). The rest of the face sits above
 * them.
 */
export function ZonesEditor({ widget, onEdit, variables }: { widget: WireWidget; onEdit: WidgetEdit; variables: readonly VariableChoice[] }) {
  const zones = widget.face?.zones ?? [];
  const [open, setOpen] = useState<number | null>(null);
  const [which, setWhich] = useState<"icon" | "iconOn">("icon");
  const setAll = (next: Zone[]) => onEdit((config, id) => editFace(config, id, "", (face) => setZones(face, next)));
  const update = (index: number, change: Partial<Zone>) => setAll(zones.map((z, at) => (at === index ? { ...z, ...change } : z)));

  return (
    <section className="flex flex-col gap-2.5">
      <div className="flex items-baseline justify-between gap-3">
        <Label>Buttons along the bottom</Label>
        <span className="text-[11px] text-faint">What each does: under Does</span>
      </div>
      {zones.map((zone, index) => (
        <div key={index} className="flex flex-col gap-2 rounded-xl border border-line bg-plate p-2">
          <div className="flex items-center gap-2">
            <button
              type="button"
              aria-label={`${zone.name}'s icon`}
              aria-expanded={open === index}
              onClick={() => setOpen(open === index ? null : index)}
              className="flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-lg border border-line text-ink-2 hover:border-signal"
            >
              {zone.icon ? <Icon name={zone.icon} size={16} decorative /> : <Glyph name="plus" size={11} />}
            </button>
            <DraftInput
              aria-label="Button name"
              value={zone.name}
              onCommit={(name) => {
                const clean = name.toLowerCase().replace(/[^a-z0-9_-]/g, "").slice(0, 24);
                if (!clean || zones.some((z, at) => at !== index && z.name === clean)) return;
                onEdit((config, id) => renameZone(config, id, zone.name, clean));
              }}
              className={cn(control, "h-8 flex-1 font-mono")}
            />
            <button
              type="button"
              aria-label={`Remove ${zone.name}`}
              onClick={() => {
                onEdit((config, id) => renameZone(config, id, zone.name, null));
                setOpen(null);
              }}
              className="text-muted hover:text-bad"
            >
              <Glyph name="close" size={12} />
            </button>
          </div>
          {open === index ? (
            <div className="flex flex-col gap-2">
              <label className="flex items-center gap-2 text-[12px] text-muted">
                <span className="w-24 shrink-0">Other icon while</span>
                <ExpressionInput
                  label="Other icon while"
                  value={zone.when ?? ""}
                  variables={variables}
                  placeholder="media.current.playing"
                  onCommit={(when) => update(index, { when: when.trim() || undefined })}
                  className="h-8 flex-1"
                />
              </label>
              {zone.when ? (
                <Segmented
                  size="sm"
                  label="Which icon"
                  value={which}
                  onChange={setWhich}
                  options={[
                    { value: "icon", label: "Its icon" },
                    { value: "iconOn", label: "While it holds" },
                  ]}
                />
              ) : null}
              <IconPicker
                value={(which === "iconOn" && zone.when ? zone.iconOn : zone.icon) ?? undefined}
                onPick={(icon) => update(index, which === "iconOn" && zone.when ? { iconOn: icon ?? undefined } : { icon: icon ?? undefined })}
              />
            </div>
          ) : null}
        </div>
      ))}
      <div className="flex flex-wrap gap-1.5">
        <button
          type="button"
          disabled={zones.length >= 5}
          onClick={() => {
            setAll([...zones, { name: zoneName(zones), icon: "star" }]);
            setOpen(zones.length);
          }}
          className="flex cursor-pointer items-center gap-1 rounded-lg border border-dashed border-line-strong px-2 py-1.5 text-[12px] text-muted hover:border-signal hover:text-signal disabled:opacity-50"
        >
          <Glyph name="plus" size={11} /> Add a button
        </button>
        {zones.length === 0 ? (
          <button
            type="button"
            onClick={() => setAll([...MUSIC_ZONES])}
            className="flex cursor-pointer items-center gap-1 rounded-lg border border-dashed border-line-strong px-2 py-1.5 text-[12px] text-muted hover:border-signal hover:text-signal"
          >
            <Glyph name="plus" size={11} /> Previous · play · next
          </button>
        ) : null}
      </div>
    </section>
  );
}
