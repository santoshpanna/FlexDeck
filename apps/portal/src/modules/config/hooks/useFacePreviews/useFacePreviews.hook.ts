import { useResource } from "@questorylabs/qhttp/react";
import { useCallback, useContext, useEffect, useMemo, useState } from "react";

import { useLive } from "../../../../hooks/useLive/useLive.hook.ts";
import { ApiContext } from "../../../../providers/ApiProvider/ApiProvider.provider.tsx";
import type { WidgetPreview, WireWidget } from "../../../../services/api/api.ts";
import type { LiveMessage } from "../../../../services/api/live.ts";
import { overlayPreviews } from "../../lib/resource.lib.ts";

/**
 * What each widget on a page shows now, as the agent works it out — so the
 * editor's preview fills a template exactly as the tablet will, and a
 * template that does not parse says why.
 *
 * Asked once when the widgets change (a moment after, so typing is not a
 * request per key) and whenever the live socket (re)opens. After that, every
 * look the agent pushes — a scene going live, a timer ticking — lands the
 * moment it changes, exactly as it does on the tablet. Nothing is polled.
 */
export function useFacePreviews(desktopId: string | null, widgets: readonly WireWidget[]): Record<string, WidgetPreview> {
  // Read without the throwing hook: a view drawn outside the API provider simply
  // has no previews.
  const api = useContext(ApiContext);
  // What the agent pushed since the last ask. Kept apart, and laid over the
  // answer: a push can land before the answer does, and is the newer of the two.
  const [pushed, setPushed] = useState<Record<string, NonNullable<WidgetPreview["state"]>>>({});
  const key = JSON.stringify(widgets);
  const [asked, setAsked] = useState<{ desktopId: string | null; key: string; widgets: readonly WireWidget[] }>({
    desktopId: null,
    key: "[]",
    widgets: [],
  });

  useEffect(() => {
    const first = setTimeout(() => {
      // From here on, only pushes newer than this ask count.
      setPushed({});
      setAsked({ desktopId, key, widgets });
    }, 250);
    return () => clearTimeout(first);
    // `key` stands for `widgets`: a new array with the same widgets is no change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [desktopId, key]);

  const enabled = Boolean(api) && asked.desktopId !== null && asked.desktopId === desktopId && asked.widgets.length > 0;
  const load = useCallback(
    // An agent that predates previews, or a dropped call: draw from the config.
    async (): Promise<Record<string, WidgetPreview>> =>
      api ? api.facePreviews(asked.desktopId ?? "", [...asked.widgets]).catch(() => ({})) : {},
    [api, asked],
  );
  const answer = useResource<Record<string, WidgetPreview>>({
    id: [asked.desktopId, "previews", asked.key],
    load,
    when: enabled,
  });
  const [shown, setShown] = useState<{ desktopId: string | null; previews: Record<string, WidgetPreview> } | null>(null);
  if (enabled && answer.value && shown?.previews !== answer.value) {
    setShown({ desktopId: asked.desktopId, previews: answer.value });
  }
  const previews = !enabled
    ? undefined
    : (answer.value ?? (shown?.desktopId === desktopId ? shown.previews : undefined));

  const hear = useCallback((message: LiveMessage) => {
    if (message.kind === "open") {
      setPushed({});
      return;
    }
    if (message.kind !== "state") return;
    setPushed((current) => ({ ...current, [message.widgetId]: message.state }));
  }, []);
  useLive(desktopId, hear);

  return useMemo(
    () => overlayPreviews(previews, pushed, widgets.map((widget) => widget.id)),
    // `key` stands for `widgets`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [previews, pushed, key],
  );
}
