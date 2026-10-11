import { useContext, useEffect, useState } from "react";

import { Label, cn } from "@flexdeck/ui";

import { ApiContext } from "../../../../providers/ApiProvider/ApiProvider.provider.tsx";
import { DesktopContext } from "../../../../providers/DesktopProvider/DesktopProvider.provider.tsx";
import type { DataSource, WireConfig, WireWidget } from "../../../../services/api/api.ts";
import { setKeySource } from "../../lib/face.edit.lib.ts";
import { keptSecret } from "../../lib/sources.lib.ts";
import { DraftInput, control } from "../Flow/fields.tsx";

type WidgetEdit = (transform: (config: WireConfig, widgetId: string) => WireConfig) => void;

/**
 * A key's own run of a data source: which source, and this key's settings
 * for it — Tokyo here, Berlin on the key beside it. The key then reads its
 * values as `{{this.<field>}}`.
 */
export function KeySource({ widget, onEdit }: { widget: WireWidget; onEdit: WidgetEdit }) {
  const api = useContext(ApiContext);
  const desktopId = useContext(DesktopContext)?.selected?.id ?? null;
  const [sources, setSources] = useState<DataSource[]>([]);
  const [secrets, setSecrets] = useState<string[]>([]);
  const [asked, setAsked] = useState(0);
  const use = widget.sources?.[0];
  // A secret typed here leaves the config when it is saved; ask again then.
  const pending = JSON.stringify(use?.args ?? {});
  useEffect(() => {
    if (!api || !desktopId) return;
    let live = true;
    void api
      .sources(desktopId)
      .then((data) => live && setSecrets(data.secrets ?? []))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [api, desktopId, pending, asked]);
  useEffect(() => {
    if (!api || !desktopId) return;
    let live = true;
    void Promise.all([api.sources(desktopId).catch(() => ({ sources: [] })), api.pluginCatalogue(desktopId).catch(() => ({ plugins: [] }))]).then(
      ([data, catalogue]) => {
        if (!live) return;
        // A plugin's feed reads like a data source: `<plugin>/<feed>`.
        const feeds: DataSource[] = catalogue.plugins.flatMap((plugin) =>
          (plugin.feeds ?? []).map((feed) => ({
            id: `${plugin.id}/${feed.name}`,
            name: `${plugin.name}: ${feed.description || feed.name}`,
            args: (feed.arguments ?? []).map((a) => ({ name: a.name, default: a.example ?? "", description: a.description ?? "" })),
            fields: (feed.fields ?? []).map((f) => ({ name: f.name, value: "" })),
          })),
        );
        const fromRecipes = Object.values((data as { recipes?: Record<string, DataSource[]> }).recipes ?? {}).flat();
        setSources([...(data.sources ?? []), ...fromRecipes, ...feeds]);
      },
    );
    return () => {
      live = false;
    };
  }, [api, desktopId]);

  const chosen = sources.find((source) => source.id === use?.source);
  const args = use?.args ?? {};
  const set = (source: string, nextArgs: Record<string, string>) => onEdit((config, id) => setKeySource(config, id, source ? { source, args: nextArgs } : null));

  if (sources.length === 0 && !use) return null;

  return (
    <section className="flex flex-col gap-2.5">
      <div className="flex items-baseline justify-between gap-3">
        <Label>Its own data</Label>
        <span className="text-[11px] text-faint">Read as {"{{this.field}}"}</span>
      </div>
      <select aria-label="Data source" value={use?.source ?? ""} onChange={(event) => set(event.target.value, {})} className={cn(control, "h-9")}>
        <option value="">None</option>
        {sources.map((source) => (
          <option key={source.id} value={source.id}>
            {source.name || source.id}
          </option>
        ))}
        {use && !chosen ? <option value={use.source}>{use.source} (not defined)</option> : null}
      </select>
      {chosen?.args?.map((arg) => {
        const kept = arg.secret ? keptSecret(secrets, chosen.id, widget.id, arg.name) : null;
        const typed = Boolean(args[arg.name]);
        return (
          <div key={arg.name} className="flex flex-col gap-1">
            <div className="flex items-center gap-2">
              <span className="w-20 shrink-0 truncate text-[12px] text-muted" title={arg.description}>
                {arg.description || arg.name}
              </span>
              <DraftInput
                aria-label={arg.name}
                type={arg.secret ? "password" : "text"}
                autoComplete="off"
                value={args[arg.name] ?? ""}
                placeholder={
                  arg.secret
                    ? kept === "own"
                      ? "Kept on this computer"
                      : kept === "shared"
                        ? "Using the one already given"
                        : "Not set"
                    : arg.default
                }
                onCommit={(value) => set(chosen.id, { ...args, [arg.name]: value })}
                className={cn(control, "h-8 flex-1")}
              />
            </div>
            {arg.secret ? (
              <p className="m-0 flex items-center gap-2 pl-22 text-[11px] text-faint">
                {typed
                  ? "Kept on this computer once saved — never in the deck or anything you share."
                  : kept === "own"
                    ? "Kept on this computer. Type to replace it."
                    : kept === "shared"
                      ? "The first one given is used by every key of this source until it has its own."
                      : "Secret: kept on this computer only."}
                {kept === "own" && !typed ? (
                  <button
                    type="button"
                    onClick={() => {
                      if (!api || !desktopId) return;
                      void api.forgetSourceSecret(desktopId, chosen.id, widget.id, arg.name).then(() => setAsked((n) => n + 1));
                    }}
                    className="text-muted hover:text-bad"
                  >
                    Forget
                  </button>
                ) : null}
              </p>
            ) : null}
          </div>
        );
      })}
      {chosen ? (
        <p className="m-0 text-[11px] text-faint">
          Fields: {(chosen.fields ?? []).map((field) => `this.${field.name}`).join(", ") || "none yet"}
        </p>
      ) : null}
    </section>
  );
}
